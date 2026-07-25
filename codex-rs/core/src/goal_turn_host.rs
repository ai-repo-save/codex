use std::future::Future;
use std::sync::Weak;

use codex_extension_api::GoalTurnHost;
use codex_extension_api::GoalTurnHostRejection;
use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;

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
}

impl GoalTurnHost for GoalTurnHostAdapter {
    fn start_goal_turn_if_idle(
        &self,
        thread_id: ThreadId,
        items: Vec<ResponseItem>,
    ) -> impl Future<Output = Result<(), GoalTurnHostRejection>> + Send {
        let thread_manager = self.thread_manager.clone();
        async move {
            let Some(thread_manager) = thread_manager.upgrade() else {
                return Err(GoalTurnHostRejection::HostUnavailable);
            };
            let thread = thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|_| GoalTurnHostRejection::ThreadUnavailable)?;
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
            let Some(thread_manager) = thread_manager.upgrade() else {
                return Err(GoalTurnHostRejection::HostUnavailable);
            };
            let thread = thread_manager
                .get_thread(thread_id)
                .await
                .map_err(|_| GoalTurnHostRejection::ThreadUnavailable)?;
            thread
                .inject_if_running(items)
                .await
                .map_err(|_| GoalTurnHostRejection::NoActiveTurn)
        }
    }
}
