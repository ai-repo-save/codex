use super::*;
use crate::agent::inspect::InspectedAgent;
use crate::tools::handlers::multi_agents_spec::create_inspect_agent_tool;
use codex_tools::ToolSpec;

pub(crate) struct Handler;

#[async_trait::async_trait]
impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("inspect_agent")
    }

    fn spec(&self) -> ToolSpec {
        create_inspect_agent_tool()
    }

    async fn handle(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            ..
        } = invocation;
        let arguments = function_arguments(payload)?;
        let args: InspectAgentArgs = parse_arguments(&arguments)?;
        let agent_id = resolve_agent_target(&session, &turn, &args.target).await?;
        let inspected_agent = session
            .services
            .agent_control
            .inspect_agent(agent_id, args.tail_items)
            .await
            .map_err(collab_spawn_error)?;

        Ok(boxed_tool_output(InspectAgentResult { inspected_agent }))
    }
}

impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectAgentArgs {
    target: String,
    tail_items: Option<usize>,
}

#[derive(Debug, Serialize)]
pub(crate) struct InspectAgentResult {
    #[serde(flatten)]
    inspected_agent: InspectedAgent,
}

impl ToolOutput for InspectAgentResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, "inspect_agent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, Some(true), "inspect_agent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "inspect_agent")
    }
}
