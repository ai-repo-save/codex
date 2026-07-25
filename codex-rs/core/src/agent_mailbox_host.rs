use std::future::Future;
use std::sync::Weak;

use codex_extension_api::AgentMailboxHost;
use codex_extension_api::AgentMailboxHostError;
use codex_extension_api::AgentMailboxTarget;
use codex_protocol::ThreadId;

use crate::ThreadManager;

/// Core adapter that exposes live-agent lookup and count-only wait wakes.
///
/// The adapter deliberately retains only a weak thread-manager reference, so
/// extension runtime state cannot keep the process-wide thread manager alive.
#[derive(Clone, Debug)]
pub struct AgentMailboxHostAdapter {
    thread_manager: Weak<ThreadManager>,
}

impl AgentMailboxHostAdapter {
    /// Creates an adapter over the host's process-scoped thread manager.
    pub fn new(thread_manager: Weak<ThreadManager>) -> Self {
        Self { thread_manager }
    }
}

impl AgentMailboxHost for AgentMailboxHostAdapter {
    fn resolve_target(
        &self,
        current_thread_id: ThreadId,
        target: &str,
    ) -> impl Future<Output = Result<AgentMailboxTarget, AgentMailboxHostError>> + Send {
        let thread_manager = self.thread_manager.clone();
        let target = target.to_string();
        async move {
            let Some(thread_manager) = thread_manager.upgrade() else {
                return Err(AgentMailboxHostError::HostUnavailable);
            };
            thread_manager
                .resolve_agent_mailbox_target(current_thread_id, &target)
                .await
                .map_err(|err| AgentMailboxHostError::OperationRejected(err.to_string()))
        }
    }

    fn notify_activity(
        &self,
        recipient_thread_id: ThreadId,
    ) -> impl Future<Output = Result<(), AgentMailboxHostError>> + Send {
        let thread_manager = self.thread_manager.clone();
        async move {
            let Some(thread_manager) = thread_manager.upgrade() else {
                return Err(AgentMailboxHostError::HostUnavailable);
            };
            thread_manager
                .notify_agent_mailbox_activity(recipient_thread_id)
                .await
                .map_err(|err| AgentMailboxHostError::OperationRejected(err.to_string()))
        }
    }
}
