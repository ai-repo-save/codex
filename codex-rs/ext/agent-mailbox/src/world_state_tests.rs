use codex_extension_api::PreviousWorldStateSection;
use codex_state::AgentMailboxUnreadSnapshot;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::mailbox_world_state_section;

const BODY_WITH_UNREAD_MESSAGES: &str = "Agent mailbox: 3 unread — 1 action required, 1 result, 1 progress. Use agent_mailbox.read to process them.";
const EMPTY_MAILBOX_BODY: &str = "Agent mailbox: 0 unread.";

#[test]
fn world_state_exposes_only_aggregate_counts_and_replaces_empty_mailbox() {
    let unread = AgentMailboxUnreadSnapshot {
        total: 3,
        progress: 1,
        result: 1,
        action_required: 1,
        revision: 7,
    };
    let unread_section = mailbox_world_state_section(unread.clone());

    assert_eq!(
        json!({
            "total": 3,
            "progress": 1,
            "result": 1,
            "actionRequired": 1,
            "revision": 7,
        }),
        *unread_section.snapshot()
    );
    assert_eq!(
        Some(BODY_WITH_UNREAD_MESSAGES.to_string()),
        unread_section
            .render_diff(PreviousWorldStateSection::Absent)
            .map(|fragment| fragment.body().to_string())
    );

    let empty_section = mailbox_world_state_section(AgentMailboxUnreadSnapshot {
        total: 0,
        progress: 0,
        result: 0,
        action_required: 0,
        revision: 8,
    });
    assert_eq!(
        Some(EMPTY_MAILBOX_BODY.to_string()),
        empty_section
            .render_diff(PreviousWorldStateSection::Known(unread_section.snapshot()))
            .map(|fragment| fragment.body().to_string())
    );
}
