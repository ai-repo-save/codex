//! Durable, explicitly read agent mailbox extension.

mod extension;
mod output;
mod schema;
mod tools;
mod world_state;

pub use extension::AgentMailboxExtension;
pub use extension::AgentMailboxExtensionConfig;
pub use extension::AgentMailboxStatusNotifier;
pub use extension::NoopAgentMailboxStatusNotifier;
pub use extension::install_with_backend;

pub const AGENT_MAILBOX_NAMESPACE: &str = "agent_mailbox";
pub const READ_TOOL_NAME: &str = "read";
pub const SEND_TOOL_NAME: &str = "send";

/// Limits one stored mailbox body so its explicitly-read model input remains bounded.
pub(crate) const MAX_AGENT_MAILBOX_PAYLOAD_BYTES: usize = 4 * 1024;
/// Bounds one message's rendered tool-output content items, including metadata.
pub(crate) const MAX_AGENT_MAILBOX_SINGLE_OUTPUT_BYTES: usize = 4_480;
/// Bounds all rendered content items for one mailbox read.
pub(crate) const MAX_AGENT_MAILBOX_READ_OUTPUT_BYTES: usize = 9 * 1024;
/// Limits a read to the number of validated messages that fit its output budget.
pub(crate) const MAX_AGENT_MAILBOX_READ_MESSAGES: usize = 2;
