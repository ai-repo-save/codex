//! Context-window reminder evaluation and recording.
//!
//! Kept out of `session/mod.rs` so fork-owned reminder policy does not expand the
//! upstream session orchestration hot file.

use codex_protocol::protocol::TokenUsageInfo;

use crate::context::ContextReminder;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextReminderStatus {
    pub(crate) remaining_percent: Option<i64>,
    pub(crate) used_tokens: i64,
    pub(crate) percent_threshold_active: bool,
    pub(crate) used_tokens_threshold_active: bool,
}

pub(crate) fn context_reminder_status(
    turn_context: &TurnContext,
    token_info: &TokenUsageInfo,
) -> ContextReminderStatus {
    let remaining_percent = turn_context.model_context_window().map(|context_window| {
        token_info
            .last_token_usage
            .percent_of_context_window_remaining(context_window)
    });
    let used_tokens = token_info.last_token_usage.total_tokens;
    let percent_threshold_active = turn_context.config.context_reminder.enabled
        && remaining_percent
            .is_some_and(|value| value <= turn_context.config.context_reminder.remaining_percent);
    let used_tokens_threshold_active = turn_context.config.context_reminder.enabled
        && turn_context
            .config
            .context_reminder
            .used_tokens
            .is_some_and(|threshold| used_tokens >= threshold);

    ContextReminderStatus {
        remaining_percent,
        used_tokens,
        percent_threshold_active,
        used_tokens_threshold_active,
    }
}

impl Session {
    pub(crate) async fn record_context_reminder(
        &self,
        turn_context: &TurnContext,
        status: ContextReminderStatus,
    ) {
        let Some(reminder_message) =
            crate::context_manager::updates::build_developer_update_item(vec![
                ContextReminder::new(
                    status.remaining_percent,
                    status.used_tokens,
                    turn_context.config.context_reminder.used_tokens,
                    &turn_context.config.context_reminder.message,
                )
                .render(),
            ])
        else {
            return;
        };
        self.record_conversation_items(turn_context, std::slice::from_ref(&reminder_message))
            .await;
    }
}
