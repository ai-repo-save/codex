use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;

/// Why the host could not apply a goal's requested turn operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalTurnHostRejection {
    /// The host that owns live threads is no longer available.
    HostUnavailable,
    /// The goal's thread is not currently live.
    ThreadUnavailable,
    /// The thread cannot start automatic work while it is idle.
    IdleTurnRejected,
    /// The thread has no active turn that can accept injected input.
    NoActiveTurn,
}

/// Host operations that the goal extension needs to continue or steer a goal.
///
/// The extension owns goal state and chooses the model-visible items. The host
/// owns live-thread lookup, turn admission, and same-turn input injection.
pub trait GoalTurnHost: Send + Sync {
    /// Confirms that the target thread remains available for goal work.
    fn ensure_goal_thread_available(
        &self,
        thread_id: ThreadId,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send;

    /// Starts an automatic goal continuation when the target thread is idle.
    fn start_goal_turn_if_idle(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send;

    /// Injects goal steering into a currently active turn.
    fn inject_goal_turn_items(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send;
}

/// Erased future returned by a goal-turn host operation.
pub type GoalTurnHostFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), GoalTurnHostRejection>> + Send + 'a>>;

trait ErasedGoalTurnHost: Send + Sync {
    fn ensure_goal_thread_available(&self, thread_id: ThreadId) -> GoalTurnHostFuture<'_>;

    fn start_goal_turn_if_idle(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> GoalTurnHostFuture<'_>;

    fn inject_goal_turn_items(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> GoalTurnHostFuture<'_>;
}

impl<H> ErasedGoalTurnHost for H
where
    H: GoalTurnHost,
{
    fn ensure_goal_thread_available(&self, thread_id: ThreadId) -> GoalTurnHostFuture<'_> {
        Box::pin(GoalTurnHost::ensure_goal_thread_available(self, thread_id))
    }

    fn start_goal_turn_if_idle(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> GoalTurnHostFuture<'_> {
        Box::pin(GoalTurnHost::start_goal_turn_if_idle(self, thread_id, items))
    }

    fn inject_goal_turn_items(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> GoalTurnHostFuture<'_> {
        Box::pin(GoalTurnHost::inject_goal_turn_items(self, thread_id, items))
    }
}

/// Erased handle that lets extensions retain a typed [`GoalTurnHost`] without
/// exposing the host implementation in extension state.
#[derive(Clone)]
pub struct GoalTurnHostHandle {
    inner: Arc<dyn ErasedGoalTurnHost>,
}

impl GoalTurnHostHandle {
    /// Wraps one host implementation for storage by an extension runtime.
    pub fn new(host: impl GoalTurnHost + 'static) -> Self {
        Self {
            inner: Arc::new(host),
        }
    }

    /// Confirms that the target thread remains available for goal work.
    pub fn ensure_goal_thread_available(&self, thread_id: ThreadId) -> GoalTurnHostFuture<'_> {
        self.inner.ensure_goal_thread_available(thread_id)
    }

    /// Starts automatic goal work through the host-owned admission path.
    pub fn start_goal_turn_if_idle(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> GoalTurnHostFuture<'_> {
        self.inner.start_goal_turn_if_idle(thread_id, items)
    }

    /// Injects model-visible goal items through the host-owned active turn.
    pub fn inject_goal_turn_items(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> GoalTurnHostFuture<'_> {
        self.inner.inject_goal_turn_items(thread_id, items)
    }
}

/// Host used by extensions when no live-thread implementation is available.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopGoalTurnHost;

impl GoalTurnHost for NoopGoalTurnHost {
    fn ensure_goal_thread_available(
        &self,
        _thread_id: ThreadId,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        std::future::ready(Err(GoalTurnHostRejection::HostUnavailable))
    }

    fn start_goal_turn_if_idle(
        &self,
        _thread_id: ThreadId,
        _items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        std::future::ready(Err(GoalTurnHostRejection::HostUnavailable))
    }

    fn inject_goal_turn_items(
        &self,
        _thread_id: ThreadId,
        _items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        std::future::ready(Err(GoalTurnHostRejection::HostUnavailable))
    }
}
