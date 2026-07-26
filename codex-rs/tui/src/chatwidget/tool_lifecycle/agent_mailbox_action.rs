//! Agent mailbox action transcript lifecycle.

use super::super::*;

impl ChatWidget {
    pub(in crate::chatwidget) fn on_agent_mailbox_action_started(&mut self, item: ThreadItem) {
        if let Some(cell) = crate::agent_mailbox_action::agent_mailbox_action_history_cell(&item) {
            self.flush_answer_stream_with_separator();
            self.flush_active_cell();
            self.transcript.active_cell = Some(Box::new(cell));
            self.bump_active_cell_revision();
            self.request_redraw();
        }
    }

    pub(in crate::chatwidget) fn on_agent_mailbox_action_completed(&mut self, item: ThreadItem) {
        let Some(completed_cell) =
            crate::agent_mailbox_action::agent_mailbox_action_history_cell(&item)
        else {
            return;
        };

        self.flush_answer_stream_with_separator();
        let mut handled = false;
        if let Some(active_cell) = self.transcript.active_cell.as_mut().and_then(|cell| {
            cell.as_any_mut()
                .downcast_mut::<crate::agent_mailbox_action::AgentMailboxActionCell>()
        }) && active_cell.id() == completed_cell.id()
        {
            active_cell.update(completed_cell.action().clone());
            self.bump_active_cell_revision();
            self.flush_active_cell();
            handled = true;
        }

        if !handled {
            self.transcript.needs_final_message_separator = true;
            self.app_event_tx
                .send(AppEvent::InsertHistoryCell(Box::new(completed_cell)));
        }
        self.transcript.had_work_activity = true;
        self.request_redraw();
    }
}
