use super::*;
use crate::agent::inspect::InspectedAgent;
use codex_agent_control::create_inspect_agent_tool;
use codex_tools::ToolSpec;

pub(crate) struct Handler;

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("inspect_agent")
    }

    fn spec(&self) -> ToolSpec {
        create_inspect_agent_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl Handler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            call_id,
            ..
        } = invocation;
        let arguments = function_arguments(payload)?;
        let args: InspectAgentArgs = parse_arguments(&arguments)?;
        let agent_id = resolve_agent_target(&session, &turn, &args.target).await?;
        let receiver_agent = session
            .services
            .agent_control
            .ensure_agent_known(agent_id)
            .map_err(|err| collab_agent_error(agent_id, err))?;
        let receiver_agent_path = receiver_agent.agent_path.clone().ok_or_else(|| {
            FunctionCallError::RespondToModel("target agent is missing an agent_path".to_string())
        })?;
        let result = session
            .services
            .agent_control
            .inspect_agent(agent_id, args.tail_items)
            .await
            .map_err(collab_spawn_error);
        emit_sub_agent_interaction(
            &session,
            &turn,
            call_id,
            agent_id,
            receiver_agent_path,
            SubAgentActivityOperation::InspectAgent,
            if result.is_ok() {
                SubAgentActivityOutcome::Succeeded
            } else {
                SubAgentActivityOutcome::Failed
            },
        )
        .await;
        let inspected_agent = result?;

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
