use crate::agent::AgentControl;
use crate::agent::AgentStatus;
use crate::session_prefix::format_inter_agent_completion_message;
use codex_protocol::AgentPath;
use codex_protocol::ResponseItemId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::InterAgentCommunication;

/// Builds the standard terminal notification and lets an enabled extension capture it first.
///
/// `None` means either that the agent path/status cannot produce a terminal notification or that
/// an extension durably captured it. Callers must only perform legacy parent delivery for a
/// returned communication.
pub(crate) async fn prepare_terminal_delivery(
    control: &AgentControl,
    sender_thread_id: ThreadId,
    recipient_thread_id: ThreadId,
    sender_agent_path: AgentPath,
    status: &AgentStatus,
    terminal_message_id_suffix: String,
) -> Option<InterAgentCommunication> {
    let parent_agent_path = sender_agent_path
        .as_str()
        .rsplit_once('/')
        .and_then(|(parent, _)| AgentPath::try_from(parent).ok())?;
    let message = format_inter_agent_completion_message(
        parent_agent_path.clone(),
        sender_agent_path.clone(),
        status,
    )?;
    let mut communication = InterAgentCommunication::new(
        sender_agent_path,
        parent_agent_path,
        Vec::new(),
        message,
        /*trigger_turn*/ false,
    );
    if !control
        .terminal_message_capture_enabled(recipient_thread_id)
        .await
    {
        return Some(communication);
    }

    communication.id = Some(ResponseItemId::with_suffix(
        "agent_terminal",
        terminal_message_id_suffix,
    ));
    if control
        .try_claim_terminal_message(
            sender_thread_id,
            recipient_thread_id,
            &communication,
            status,
        )
        .await
    {
        return None;
    }
    communication.id = None;
    Some(communication)
}
