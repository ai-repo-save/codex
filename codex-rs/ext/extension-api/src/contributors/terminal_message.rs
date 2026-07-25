use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::InterAgentCommunication;

use crate::ExtensionData;

/// Inputs supplied when the host is about to deliver an automatic terminal
/// message from a child agent to its direct parent.
///
/// Implementations may durably capture the communication. They must return
/// [`TerminalMessageDisposition::Committed`] only after the capture has
/// committed, because that suppresses the host's normal direct delivery path.
/// The host supplies a stable `communication.id`; implementations should use
/// it as their idempotency key.
pub struct TerminalMessageInput<'a> {
    pub session_id: SessionId,
    pub sender_thread_id: ThreadId,
    pub recipient_thread_id: ThreadId,
    pub communication: &'a InterAgentCommunication,
    pub status: &'a AgentStatus,
    pub recipient_thread_store: &'a ExtensionData,
}

/// Result of one terminal-message contribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalMessageDisposition {
    /// The contributor did not capture the message.
    Unclaimed,
    /// The contributor durably captured the message.
    Committed,
}
