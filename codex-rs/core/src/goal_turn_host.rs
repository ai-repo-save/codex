use std::future::Future;
use std::sync::Weak;

use codex_extension_api::GoalTurnHost;
use codex_extension_api::GoalTurnHostRejection;
use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;

use crate::CodexThread;
use crate::ThreadManager;

/// Core adapter that gives the goal extension access to live-thread turn APIs.
///
/// The adapter deliberately retains only a weak thread-manager reference, so
/// extension runtime state cannot keep the process-wide thread manager alive.
#[derive(Clone, Debug)]
pub struct GoalTurnHostAdapter {
    thread_manager: Weak<ThreadManager>,
}

impl GoalTurnHostAdapter {
    /// Creates an adapter over the host's process-scoped thread manager.
    pub fn new(thread_manager: Weak<ThreadManager>) -> Self {
        Self { thread_manager }
    }

    async fn live_thread(
        &self,
        thread_id: ThreadId,
    ) -> Result<std::sync::Arc<CodexThread>, GoalTurnHostRejection> {
        let Some(thread_manager) = self.thread_manager.upgrade() else {
            return Err(GoalTurnHostRejection::HostUnavailable);
        };
        thread_manager
            .get_thread(thread_id)
            .await
            .map_err(|_| GoalTurnHostRejection::ThreadUnavailable)
    }
}

impl GoalTurnHost for GoalTurnHostAdapter {
    fn ensure_goal_thread_available(
        &self,
        thread_id: ThreadId,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        async move { self.live_thread(thread_id).await.map(|_| ()) }
    }

    fn start_goal_turn_if_idle(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        let thread_manager = self.thread_manager.clone();
        async move {
            let thread = GoalTurnHostAdapter { thread_manager }
                .live_thread(thread_id)
                .await?;
            thread
                .try_start_turn_if_idle(items)
                .await
                .map_err(|_| GoalTurnHostRejection::IdleTurnRejected)
        }
    }

    fn inject_goal_turn_items(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        let thread_manager = self.thread_manager.clone();
        async move {
            let thread = GoalTurnHostAdapter { thread_manager }
                .live_thread(thread_id)
                .await?;
            thread
                .inject_if_running(items)
                .await
                .map_err(|_| GoalTurnHostRejection::NoActiveTurn)
        }
    }
}
