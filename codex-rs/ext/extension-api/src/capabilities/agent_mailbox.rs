use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_protocol::AgentPath;
use codex_protocol::ThreadId;

/// Live agent target resolved for an extension-owned mailbox operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentMailboxTarget {
    pub thread_id: ThreadId,
    pub agent_path: AgentPath,
}

/// Why the host could not complete a mailbox operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentMailboxHostError {
    /// The host that owns live agent sessions is no longer available.
    HostUnavailable,
    /// The live agent lookup or count-only wake was rejected.
    OperationRejected(String),
}

impl fmt::Display for AgentMailboxHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostUnavailable => formatter.write_str("agent thread manager is unavailable"),
            Self::OperationRejected(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for AgentMailboxHostError {}

/// Host operations needed by the durable agent mailbox extension.
///
/// The extension owns persistence, message filtering, and model-visible
/// output. The host owns live-agent reference resolution and count-only wake
/// delivery to an active wait.
pub trait AgentMailboxHost: Send + Sync {
    /// Resolves a live target using the host's native agent-reference rules.
    fn resolve_target(
        &self,
        current_thread_id: ThreadId,
        target: &str,
    ) -> impl Future<Output = Result<AgentMailboxTarget, AgentMailboxHostError>> + Send;

    /// Wakes an active wait after the durable mailbox commit succeeds.
    fn notify_activity(
        &self,
        recipient_thread_id: ThreadId,
    ) -> impl Future<Output = Result<(), AgentMailboxHostError>> + Send;
}

/// Erased future returned by an agent mailbox host operation.
pub type AgentMailboxHostFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, AgentMailboxHostError>> + Send + 'a>>;

trait ErasedAgentMailboxHost: Send + Sync {
    fn resolve_target<'a>(
        &'a self,
        current_thread_id: ThreadId,
        target: &'a str,
    ) -> AgentMailboxHostFuture<'a, AgentMailboxTarget>;

    fn notify_activity(&self, recipient_thread_id: ThreadId) -> AgentMailboxHostFuture<'_, ()>;
}

impl<H> ErasedAgentMailboxHost for H
where
    H: AgentMailboxHost,
{
    fn resolve_target<'a>(
        &'a self,
        current_thread_id: ThreadId,
        target: &'a str,
    ) -> AgentMailboxHostFuture<'a, AgentMailboxTarget> {
        Box::pin(AgentMailboxHost::resolve_target(
            self,
            current_thread_id,
            target,
        ))
    }

    fn notify_activity(&self, recipient_thread_id: ThreadId) -> AgentMailboxHostFuture<'_, ()> {
        Box::pin(AgentMailboxHost::notify_activity(self, recipient_thread_id))
    }
}

/// Erased handle retained by the mailbox extension runtime.
#[derive(Clone)]
pub struct AgentMailboxHostHandle {
    inner: Arc<dyn ErasedAgentMailboxHost>,
}

impl AgentMailboxHostHandle {
    /// Wraps one host implementation for storage by an extension.
    pub fn new(host: impl AgentMailboxHost + 'static) -> Self {
        Self {
            inner: Arc::new(host),
        }
    }

    /// Resolves a live mailbox target through the host.
    pub fn resolve_target<'a>(
        &'a self,
        current_thread_id: ThreadId,
        target: &'a str,
    ) -> AgentMailboxHostFuture<'a, AgentMailboxTarget> {
        self.inner.resolve_target(current_thread_id, target)
    }

    /// Publishes one count-only mailbox activity edge through the host.
    pub fn notify_activity(&self, recipient_thread_id: ThreadId) -> AgentMailboxHostFuture<'_, ()> {
        self.inner.notify_activity(recipient_thread_id)
    }
}

/// Host used when no live agent directory is available.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopAgentMailboxHost;

impl AgentMailboxHost for NoopAgentMailboxHost {
    fn resolve_target(
        &self,
        _current_thread_id: ThreadId,
        _target: &str,
    ) -> impl Future<Output = Result<AgentMailboxTarget, AgentMailboxHostError>> + Send {
        std::future::ready(Err(AgentMailboxHostError::HostUnavailable))
    }

    fn notify_activity(
        &self,
        _recipient_thread_id: ThreadId,
    ) -> impl Future<Output = Result<(), AgentMailboxHostError>> + Send {
        std::future::ready(Err(AgentMailboxHostError::HostUnavailable))
    }
}
