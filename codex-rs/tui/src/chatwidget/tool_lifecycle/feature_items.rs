//! Shared classification for feature-owned thread item lifecycles.

use super::super::*;

pub(in crate::chatwidget) enum FeatureItemLifecycle {
    Started,
    Status,
}

impl ChatWidget {
    pub(in crate::chatwidget) fn handle_feature_thread_item(
        &mut self,
        item: ThreadItem,
        lifecycle: FeatureItemLifecycle,
    ) -> Option<ThreadItem> {
        match item {
            item @ ThreadItem::AgentMailboxAction(_)
                if matches!(lifecycle, FeatureItemLifecycle::Started) =>
            {
                self.on_agent_mailbox_action_started(item);
            }
            item @ ThreadItem::AgentMailboxAction(codex_app_server_protocol::AgentMailboxAction {
                status: codex_app_server_protocol::AgentMailboxActionStatus::InProgress,
                ..
            }) => self.on_agent_mailbox_action_started(item),
            item @ ThreadItem::AgentMailboxAction(_) => self.on_agent_mailbox_action_completed(item),
            item @ ThreadItem::MemoryMutation(_)
                if matches!(lifecycle, FeatureItemLifecycle::Started) =>
            {
                self.on_memory_mutation_started(item);
            }
            item @ ThreadItem::MemoryMutation(codex_app_server_protocol::MemoryMutation {
                status: codex_app_server_protocol::MemoryMutationStatus::InProgress,
                ..
            }) => self.on_memory_mutation_started(item),
            item @ ThreadItem::MemoryMutation(_) => self.on_memory_mutation_completed(item),
            item @ ThreadItem::CollabAgentToolCall { .. } => {
                self.on_collab_agent_tool_call(item);
            }
            item @ ThreadItem::SubAgentActivity { .. } => self.on_sub_agent_activity(item),
            item => return Some(item),
        }
        None
    }
}
