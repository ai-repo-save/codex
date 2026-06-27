use super::*;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::ContextRewoundToAnchorEvent;
use codex_protocol::protocol::UserMessageEvent;
use pretty_assertions::assert_eq;

const ANCHOR_ID: &str = "anchor";

fn saved_anchor(anchor_id: &str, label: Option<&str>, boundary: u64, created_at: i64) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(ContextAnchorSavedEvent {
        anchor_id: anchor_id.to_string(),
        label: label.map(str::to_string),
        history_boundary: boundary,
        created_at,
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
        window_number: None,
        first_window_id: None,
        previous_window_id: None,
        window_id: None,
    })
}

fn rewind(anchor_id: &str) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(
        ContextRewoundToAnchorEvent {
            anchor_id: anchor_id.to_string(),
            dropped_turns: 1,
            note: "carry".to_string(),
        },
    ))
}

fn history_item(text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn count_user_turns_since_anchor_forgets_pre_compaction_anchor() {
    let items = vec![
        saved_anchor(ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0),
        user_message(),
        compaction(),
    ];

    let result = count_user_turns_since_anchor(&items, ANCHOR_ID);

    assert!(matches!(result, Err(CodexErr::InvalidRequest(_))));
}

#[test]
fn count_user_turns_since_anchor_uses_post_compaction_anchor() {
    let items = vec![
        saved_anchor(ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0),
        user_message(),
        compaction(),
        saved_anchor(ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0),
        user_message(),
    ];

    let result = count_user_turns_since_anchor(&items, ANCHOR_ID);

    assert_eq!(result.unwrap(), 1);
}

#[test]
fn list_context_anchors_returns_active_anchors_newest_first() {
    let rollout_items = vec![
        saved_anchor("a", Some("early"), /*boundary*/ 1, /*created_at*/ 10),
        user_message(),
        saved_anchor("b", Some("late"), /*boundary*/ 3, /*created_at*/ 20),
        user_message(),
    ];
    let current_history = vec![
        history_item("zero"),
        history_item("one"),
        history_item("two"),
        history_item("three"),
    ];

    let result = list_context_anchors_from_rollout(&rollout_items, &current_history, /*limit*/ 20);

    assert_eq!(result.current_history_items, 4);
    assert_eq!(result.active_anchor_count, 2);
    assert_eq!(result.invalidated_anchor_count, 0);
    assert_eq!(result.anchors.len(), 2);
    assert_eq!(result.anchors[0].anchor_id, "b");
    assert_eq!(result.anchors[0].label.as_deref(), Some("late"));
    assert_eq!(result.anchors[0].created_at, 20);
    assert_eq!(result.anchors[0].history_boundary, 3);
    assert_eq!(result.anchors[0].response_items_since_anchor, 1);
    assert_eq!(result.anchors[0].user_turns_since_anchor, 1);
    assert!(result.anchors[0].approx_tokens_since_anchor > 0);
    assert_eq!(result.anchors[1].anchor_id, "a");
    assert_eq!(result.anchors[1].response_items_since_anchor, 3);
    assert_eq!(result.anchors[1].user_turns_since_anchor, 2);
}

#[test]
fn list_context_anchors_omits_pre_compaction_anchors() {
    let rollout_items = vec![
        saved_anchor("old", /*label*/ None, /*boundary*/ 0, /*created_at*/ 10),
        user_message(),
        compaction(),
        saved_anchor("new", /*label*/ None, /*boundary*/ 1, /*created_at*/ 20),
    ];
    let current_history = vec![history_item("zero"), history_item("one")];

    let result = list_context_anchors_from_rollout(&rollout_items, &current_history, /*limit*/ 20);

    assert_eq!(result.active_anchor_count, 1);
    assert_eq!(result.invalidated_anchor_count, 1);
    assert_eq!(result.anchors.len(), 1);
    assert_eq!(result.anchors[0].anchor_id, "new");
}

#[test]
fn list_context_anchors_omits_anchors_after_rewound_target() {
    let rollout_items = vec![
        saved_anchor("a", /*label*/ None, /*boundary*/ 0, /*created_at*/ 10),
        user_message(),
        saved_anchor("b", /*label*/ None, /*boundary*/ 1, /*created_at*/ 20),
        user_message(),
        saved_anchor("c", /*label*/ None, /*boundary*/ 2, /*created_at*/ 30),
        rewind("b"),
        user_message(),
    ];
    let current_history = vec![
        history_item("zero"),
        history_item("one"),
        history_item("carry"),
    ];

    let result = list_context_anchors_from_rollout(&rollout_items, &current_history, /*limit*/ 20);

    assert_eq!(result.active_anchor_count, 2);
    assert_eq!(result.invalidated_anchor_count, 1);
    assert_eq!(
        result
            .anchors
            .iter()
            .map(|anchor| anchor.anchor_id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "a"]
    );
    assert_eq!(result.anchors[0].user_turns_since_anchor, 1);
    assert_eq!(result.anchors[1].user_turns_since_anchor, 2);
}
