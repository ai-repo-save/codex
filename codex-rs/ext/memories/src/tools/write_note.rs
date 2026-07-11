use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_otel::MetricsClient;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;

use crate::WRITE_NOTE_TOOL_NAME;
use crate::metrics::record_tool_call;
use crate::scoped::MemoryScope;
use crate::scoped::MemoryToolBackends;
use crate::scoped::PROJECT_MEMORY_MAINTENANCE_POLICY;
use crate::scoped::SESSION_MEMORY_MAINTENANCE_POLICY;
use crate::scoped::WriteScopedMemoryNoteResponse;

use super::backend_error_to_function_call;
use super::memory_function_tool;
use super::memory_tool_name;
use super::parse_args;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WriteNoteArgs {
    /// Scoped memory store to write to. Only session and project are supported.
    scope: WriteNoteScope,
    /// Short title used to derive a readable note filename.
    #[schemars(length(min = 1, max = 120))]
    title: String,
    /// Verbatim Markdown note to append to scoped memory.
    #[schemars(length(min = 1))]
    note: String,
}

#[derive(Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum WriteNoteScope {
    Session,
    Project,
}

impl WriteNoteScope {
    fn as_memory_scope(self) -> MemoryScope {
        match self {
            Self::Session => MemoryScope::Session,
            Self::Project => MemoryScope::Project,
        }
    }
}

#[derive(Clone)]
pub(super) struct WriteNoteTool {
    pub(super) backends: MemoryToolBackends,
    pub(super) metrics_client: Option<MetricsClient>,
}

impl ToolExecutor<ToolCall> for WriteNoteTool {
    fn tool_name(&self) -> ToolName {
        memory_tool_name(WRITE_NOTE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        memory_function_tool::<WriteNoteArgs, WriteScopedMemoryNoteResponse>(
            WRITE_NOTE_TOOL_NAME,
            &format!(
                "Create one append-only session or project scoped memory note. {SESSION_MEMORY_MAINTENANCE_POLICY} {PROJECT_MEMORY_MAINTENANCE_POLICY} To replace an existing note, write the corrected note before deleting the obsolete file."
            ),
        )
    }

    fn handle(&self, call: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

impl WriteNoteTool {
    async fn handle_call(
        &self,
        call: ToolCall,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, codex_extension_api::FunctionCallError>
    {
        let backends = self.backends.clone();
        let args: WriteNoteArgs = parse_args(&call)?;
        let memory_scope = args.scope.as_memory_scope();
        let scope = memory_scope.as_str();
        let response = backends
            .write_note(memory_scope, args.title, args.note)
            .await;
        record_tool_call(
            self.metrics_client.as_ref(),
            WRITE_NOTE_TOOL_NAME,
            scope,
            response.is_ok(),
            "not_applicable",
        );
        let response = response.map_err(backend_error_to_function_call)?;
        Ok(Box::new(JsonToolOutput::new(json!(response))))
    }
}
