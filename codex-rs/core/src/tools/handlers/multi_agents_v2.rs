//! Implements the MultiAgentV2 collaboration tool surface.

use crate::agent::AgentStatus;
use crate::agent::agent_resolver::resolve_agent_target;
use crate::context::ContextualUserFragment;
use crate::context::InterAgentMessage;
use crate::context::InterAgentMessageType;
use crate::function_tool::FunctionCallError;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::context::boxed_tool_output;
use crate::tools::handlers::multi_agents_common::*;
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExecutor;
use codex_protocol::AgentPath;
use codex_protocol::items::CollabAgentTool;
use codex_protocol::items::CollabAgentToolCallItem;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_protocol::items::SubAgentActivityItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::InterAgentCommunication;
use codex_protocol::protocol::SubAgentActivityKind;
use codex_protocol::protocol::SubAgentActivityOperation;
use codex_protocol::protocol::SubAgentActivityOutcome;
use codex_tools::ToolName;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value as JsonValue;

pub(crate) use ask_parent::Handler as AskParentHandler;
pub(crate) use followup_task::Handler as FollowupTaskHandler;
pub(crate) use inspect_agent::Handler as InspectAgentHandlerV2;
pub(crate) use interrupt_agent::Handler as InterruptAgentHandler;
pub(crate) use list_agents::Handler as ListAgentsHandler;
pub(crate) use send_message::Handler as SendMessageHandler;
pub(crate) use spawn::Handler as SpawnAgentHandler;
pub(crate) use wait::Handler as WaitAgentHandler;

mod ask_parent;
mod consult;
mod followup_task;
mod inspect_agent;
mod interrupt_agent;
mod list_agents;
mod message_tool;
mod send_message;
mod spawn;
pub(crate) mod wait;

pub(crate) async fn emit_sub_agent_activity(
    session: &crate::session::session::Session,
    turn: &crate::session::turn_context::TurnContext,
    item: SubAgentActivityItem,
) {
    let item = TurnItem::SubAgentActivity(item);
    session.emit_turn_item_started(turn, &item).await;
    session.emit_turn_item_completed(turn, item).await;
}

pub(crate) async fn emit_sub_agent_interaction(
    session: &crate::session::session::Session,
    turn: &crate::session::turn_context::TurnContext,
    id: String,
    agent_thread_id: codex_protocol::ThreadId,
    agent_path: AgentPath,
    operation: SubAgentActivityOperation,
    outcome: SubAgentActivityOutcome,
) {
    emit_sub_agent_activity(
        session,
        turn,
        SubAgentActivityItem {
            id,
            agent_thread_id,
            agent_path,
            kind: SubAgentActivityKind::Interacted,
            operation: Some(operation),
            outcome: Some(outcome),
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
        },
    )
    .await;
}

fn communication_from_tool_message(
    author: AgentPath,
    recipient: AgentPath,
    message: String,
    source: &crate::tools::context::ToolCallSource,
    trigger_turn: bool,
) -> InterAgentCommunication {
    if !matches!(
        source,
        crate::tools::context::ToolCallSource::DirectPlaintextMessage
    ) {
        return InterAgentCommunication::new_encrypted(
            author,
            recipient,
            Vec::new(),
            message,
            trigger_turn,
        );
    }
    communication_from_plaintext_message(author, recipient, message, trigger_turn)
}

fn communication_from_plaintext_message(
    author: AgentPath,
    recipient: AgentPath,
    message: String,
    trigger_turn: bool,
) -> InterAgentCommunication {
    let message_type = if trigger_turn {
        InterAgentMessageType::NewTask
    } else {
        InterAgentMessageType::Message
    };
    let content =
        InterAgentMessage::new(message_type, recipient.clone(), author.clone(), message).render();
    InterAgentCommunication::new(author, recipient, Vec::new(), content, trigger_turn)
}
