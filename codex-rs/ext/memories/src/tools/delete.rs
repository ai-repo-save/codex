use codex_extension_api::JsonToolOutput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolSpec;
use codex_otel::MetricsClient;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::DELETE_TOOL_NAME;
use crate::backend::DeleteMemoryRequest;
use crate::metrics::record_tool_call;
use crate::metrics::scope_from_path;
use crate::scoped::DeleteMemoryResponse;
use crate::scoped::GLOBAL_MEMORY_MAINTENANCE_POLICY;
use crate::scoped::MemoryScope;
use crate::scoped::MemoryToolBackends;
use crate::scoped::PROJECT_MEMORY_MAINTENANCE_POLICY;
use crate::scoped::SESSION_MEMORY_MAINTENANCE_POLICY;

use super::backend_error_to_function_call;
use super::memory_function_tool;
use super::memory_tool_name;
use super::parse_args;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DeleteArgs {
    /// Optional scope may be global, session, or project; when global memories are disabled, scope must be session or project.
    scope: Option<MemoryScope>,
    /// Relative path to an existing Codex memory file. Directories, globs, hidden paths, and path traversal are rejected.
    path: String,
}

#[derive(Clone)]
pub(super) struct DeleteTool {
    pub(super) backends: MemoryToolBackends,
    pub(super) metrics_client: Option<MetricsClient>,
}

impl ToolExecutor<ToolCall> for DeleteTool {
    fn tool_name(&self) -> ToolName {
        memory_tool_name(DELETE_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        memory_function_tool::<DeleteArgs, DeleteMemoryResponse>(
            DELETE_TOOL_NAME,
            &format!(
                "Delete an exact Codex memory file by relative path. {SESSION_MEMORY_MAINTENANCE_POLICY} {PROJECT_MEMORY_MAINTENANCE_POLICY} {GLOBAL_MEMORY_MAINTENANCE_POLICY} Optional scope may be global, session, or project; when global memories are disabled, scope must be session or project. Directories, globs, hidden paths, and path traversal are rejected."
            ),
        )
    }

    fn handle(&self, call: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(call))
    }
}

impl DeleteTool {
    async fn handle_call(
        &self,
        call: ToolCall,
    ) -> Result<Box<dyn codex_extension_api::ToolOutput>, codex_extension_api::FunctionCallError>
    {
        let backends = self.backends.clone();
        let args: DeleteArgs = parse_args(&call)?;
        let path = args.path;
        let scope = scope_from_path(path.as_str());
        let response = backends
            .delete(args.scope, DeleteMemoryRequest { path })
            .await;
        record_tool_call(
            self.metrics_client.as_ref(),
            DELETE_TOOL_NAME,
            scope,
            response.is_ok(),
            "not_applicable",
        );
        let response = response.map_err(backend_error_to_function_call)?;
        Ok(Box::new(JsonToolOutput::new(json!(response))))
    }
}
