use crate::function_tool::FunctionCallError;
use crate::goals::SetGoalRequest;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::goal_spec::EDIT_GOAL_TOOL_NAME;
use crate::tools::handlers::goal_spec::create_edit_goal_tool;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::protocol::ThreadGoalStatus;
use codex_tools::ToolName;
use codex_tools::ToolSpec;

use super::CompletionBudgetReport;
use super::EditGoalArgs;
use super::format_goal_error;
use super::goal_response;

pub struct EditGoalHandler;

impl ToolExecutor<ToolInvocation> for EditGoalHandler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain(EDIT_GOAL_TOOL_NAME)
    }

    fn spec(&self) -> ToolSpec {
        create_edit_goal_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl EditGoalHandler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            ..
        } = invocation;

        let arguments = match payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "edit_goal handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: EditGoalArgs = parse_arguments(&arguments)?;
        let Some(current_goal) = session
            .get_thread_goal()
            .await
            .map_err(|err| FunctionCallError::RespondToModel(format_goal_error(err)))?
        else {
            return Err(FunctionCallError::RespondToModel(
                "cannot edit a goal because this thread has no goal; use create_goal to start one"
                    .to_string(),
            ));
        };

        if current_goal.status != ThreadGoalStatus::Paused {
            return Err(FunctionCallError::RespondToModel(format!(
                "edit_goal can only change a paused goal; current goal status is {:?}. Pause the goal before asking the agent to revise its objective.",
                current_goal.status
            )));
        }

        let goal = session
            .set_thread_goal(
                turn.as_ref(),
                SetGoalRequest {
                    objective: Some(args.objective),
                    status: Some(if args.resume {
                        ThreadGoalStatus::Active
                    } else {
                        ThreadGoalStatus::Paused
                    }),
                    token_budget: None,
                },
            )
            .await
            .map_err(|err| FunctionCallError::RespondToModel(format_goal_error(err)))?;
        goal_response(Some(goal), CompletionBudgetReport::Omit).map(boxed_tool_output)
    }
}

impl CoreToolRuntime for EditGoalHandler {}
