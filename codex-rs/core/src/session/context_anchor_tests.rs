use super::*;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::UserMessageEvent;
use pretty_assertions::assert_eq;

const ANCHOR_ID: &str = "anchor";

fn saved_anchor(anchor_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(ContextAnchorSavedEvent {
        anchor_id: anchor_id.to_string(),
        label: None,
        history_boundary: 0,
        created_at: 0,
    }))
}

fn user_message() -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
        client_id: None,
        message: String::new(),
        images: None,
        local_images: Vec::new(),
        text_elements: Vec::new(),
        ..Default::default()
    }))
}

fn compaction() -> RolloutItem {
    RolloutItem::Compacted(CompactedItem {
        message: String::new(),
        replacement_history: Some(Vec::new()),
    })
}

#[test]
fn count_user_turns_since_anchor_forgets_pre_compaction_anchor() {
    let items = vec![saved_anchor(ANCHOR_ID), user_message(), compaction()];

    let result = count_user_turns_since_anchor(&items, ANCHOR_ID);

    assert!(matches!(result, Err(CodexErr::InvalidRequest(_))));
}

#[test]
fn count_user_turns_since_anchor_uses_post_compaction_anchor() {
    let items = vec![
        saved_anchor(ANCHOR_ID),
        user_message(),
        compaction(),
        saved_anchor(ANCHOR_ID),
        user_message(),
    ];

    let result = count_user_turns_since_anchor(&items, ANCHOR_ID);

    assert_eq!(result.unwrap(), 1);
}
