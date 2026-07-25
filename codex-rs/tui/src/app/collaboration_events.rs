//! Collaboration notification observation used by thread routing.

use super::*;

/// Extracts receiver thread ids from collaboration tool-call notifications.
pub(super) fn collab_receiver_thread_ids(
    notification: &ServerNotification,
) -> Option<&[String]> {
    match notification {
        ServerNotification::ItemStarted(notification)
        | ServerNotification::ItemCompleted(notification) => match &notification.item {
            ThreadItem::CollabAgentToolCall {
                receiver_thread_ids,
                ..
            } => Some(receiver_thread_ids),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn sub_agent_activity_item(
    notification: &ServerNotification,
) -> Option<&ThreadItem> {
    match notification {
        ServerNotification::ItemStarted(notification)
        | ServerNotification::ItemCompleted(notification) => match &notification.item {
            item @ ThreadItem::SubAgentActivity { .. } => Some(item),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn collab_receiver_is_not_found(
    notification: &ServerNotification,
    receiver_thread_id: &str,
) -> bool {
    match notification {
        ServerNotification::ItemCompleted(notification) => match &notification.item {
            ThreadItem::CollabAgentToolCall { agents_states, .. } => {
                agents_states.get(receiver_thread_id).is_some_and(|state| {
                    matches!(
                        &state.status,
                        codex_app_server_protocol::CollabAgentStatus::NotFound
                    )
                })
            }
            _ => false,
        },
        _ => false,
    }
}
