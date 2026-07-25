use super::*;
use codex_app_server_protocol::ThreadAgentMailboxUpdatedNotification;

pub(super) fn format_agent_mailbox_status(status: &AgentMailboxStatus) -> String {
    let mut categories = Vec::new();
    if status.action_required > 0 {
        categories.push(format!("action {}", status.action_required));
    }
    if status.result > 0 {
        categories.push(format!("result {}", status.result));
    }
    if status.progress > 0 {
        categories.push(format!("progress {}", status.progress));
    }
    format!("Inbox {} ({})", status.total, categories.join(", "))
}

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
