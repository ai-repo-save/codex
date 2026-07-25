use super::*;
use codex_app_server_protocol::ThreadAgentMailboxUpdatedNotification;

impl ChatWidget {
    pub(super) fn on_thread_agent_mailbox_updated(
        &mut self,
        notification: ThreadAgentMailboxUpdatedNotification,
    ) {
        let Some(thread_id) = self.thread_id else {
            return;
        };
        if thread_id.to_string() != notification.thread_id {
            return;
        }
        if !self.config.features.enabled(Feature::AgentMailbox) {
            self.agent_mailbox_status = None;
            self.refresh_status_line();
            return;
        }
        if self
            .agent_mailbox_status
            .as_ref()
            .is_some_and(|current| current.revision >= notification.mailbox.revision)
        {
            return;
        }

        self.agent_mailbox_status = Some(notification.mailbox);
        self.refresh_status_line();
    }
}
