//! Durable, explicitly read agent mailbox extension.

mod extension;
mod output;
mod schema;
mod tools;
mod world_state;

pub use extension::AgentMailboxExtension;
pub use extension::AgentMailboxStatusNotifier;
pub use extension::NoopAgentMailboxStatusNotifier;
pub use extension::install_with_backend;

pub const AGENT_MAILBOX_NAMESPACE: &str = "agent_mailbox";
pub const READ_TOOL_NAME: &str = "read";
pub const SEND_TOOL_NAME: &str = "send";
