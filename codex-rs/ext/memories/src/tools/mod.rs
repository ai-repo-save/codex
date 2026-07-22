use std::sync::Arc;

use codex_extension_api::FunctionCallError;
use codex_extension_api::ExtensionTurnItem;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_extension_api::parse_tool_input_schema;
use codex_otel::MetricsClient;
use codex_extension_items::ExtensionItem;
use codex_extension_items::memory_mutation::MemoryMutation;
use codex_extension_items::memory_mutation::MemoryMutationScope;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::default_namespace_description;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::MEMORY_TOOLS_NAMESPACE;
use crate::backend::MemoriesBackendError;
use crate::schema;
use crate::scoped::MemoryToolBackends;

mod ad_hoc_note;
mod delete;
mod list;
mod read;
mod search;
mod write_note;

pub(crate) fn memory_tools(
    backends: MemoryToolBackends,
    metrics_client: Option<MetricsClient>,
) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
    let mut tools: Vec<Arc<dyn ToolExecutor<ToolCall>>> = Vec::new();
    if backends.has_global() {
        tools.push(Arc::new(ad_hoc_note::AddAdHocNoteTool {
            backends: backends.clone(),
            metrics_client: metrics_client.clone(),
        }));
    }
    if backends.has_scoped() {
        tools.push(Arc::new(write_note::WriteNoteTool {
            backends: backends.clone(),
            metrics_client: metrics_client.clone(),
        }));
    }
    tools.push(Arc::new(delete::DeleteTool {
        backends: backends.clone(),
        metrics_client: metrics_client.clone(),
    }));
    tools.push(Arc::new(list::ListTool {
        backends: backends.clone(),
        metrics_client: metrics_client.clone(),
    }));
    tools.push(Arc::new(read::ReadTool {
        backends: backends.clone(),
        metrics_client: metrics_client.clone(),
    }));
    tools.push(Arc::new(search::SearchTool {
        backends,
        metrics_client,
    }));
    tools
}

pub(super) fn memory_tool_name(name: &str) -> ToolName {
    ToolName::namespaced(MEMORY_TOOLS_NAMESPACE, name)
}

pub(super) fn memory_mutation_turn_item(item: MemoryMutation) -> ExtensionTurnItem {
    ExtensionTurnItem {
        item: ExtensionItem::MemoryMutation(item),
        legacy_events: Vec::new(),
    }
}

pub(super) fn memory_mutation_scope(scope: MemoryScope) -> MemoryMutationScope {
    match scope {
        MemoryScope::Global => MemoryMutationScope::Global,
        MemoryScope::Session => MemoryMutationScope::Session,
        MemoryScope::Project => MemoryMutationScope::Project,
    }
}

pub(super) fn memory_function_tool<I: JsonSchema, O: JsonSchema>(
    name: &str,
    description: &str,
) -> ToolSpec {
    let tool = ResponsesApiTool {
        name: name.to_string(),
        description: description.to_string(),
        strict: false,
        defer_loading: None,
        parameters: parse_tool_input_schema(&schema::input_schema_for::<I>())
            .unwrap_or_else(|err| panic!("generated input schema for {name} should parse: {err}")),
        output_schema: Some(schema::output_schema_for::<O>()),
    };

    ToolSpec::Namespace(ResponsesApiNamespace {
        name: MEMORY_TOOLS_NAMESPACE.to_string(),
        description: default_namespace_description(MEMORY_TOOLS_NAMESPACE),
        tools: vec![ResponsesApiNamespaceTool::Function(tool)],
    })
}

fn parse_args<T: for<'de> Deserialize<'de>>(call: &ToolCall) -> Result<T, FunctionCallError> {
    let arguments = call.function_arguments()?;
    let value = if arguments.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(arguments)
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?
    };
    serde_json::from_value(value).map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

fn clamp_max_results(requested: Option<usize>, default: usize, max: usize) -> usize {
    requested.unwrap_or(default).clamp(1, max)
}

fn backend_error_to_function_call(err: MemoriesBackendError) -> FunctionCallError {
    match err {
        MemoriesBackendError::InvalidPath { .. }
        | MemoriesBackendError::InvalidCursor { .. }
        | MemoriesBackendError::InvalidFilename { .. }
        | MemoriesBackendError::NotFound { .. }
        | MemoriesBackendError::InvalidLineOffset
        | MemoriesBackendError::InvalidMaxLines
        | MemoriesBackendError::LineOffsetExceedsFileLength
        | MemoriesBackendError::NotFile { .. }
        | MemoriesBackendError::EmptyQuery
        | MemoriesBackendError::EmptyAdHocNote
        | MemoriesBackendError::AdHocNoteAlreadyExists { .. }
        | MemoriesBackendError::InvalidMatchWindow => {
            FunctionCallError::RespondToModel(err.to_string())
        }
        MemoriesBackendError::Io(_) => FunctionCallError::Fatal(err.to_string()),
    }
}
