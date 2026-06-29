use super::*;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::ContextRewoundToAnchorEvent;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::TurnContextItem;
use codex_protocol::protocol::UserMessageEvent;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;

const ANCHOR_ID: &str = "anchor";

fn saved_anchor(
    anchor_id: &str,
    label: Option<&str>,
    boundary: u64,
    created_at: i64,
) -> RolloutItem {
    saved_anchor_with_mode(
        anchor_id,
        label,
        boundary,
        created_at,
        Some(ModeKind::Default),
    )
}

fn saved_anchor_with_mode(
    anchor_id: &str,
    label: Option<&str>,
    boundary: u64,
    created_at: i64,
    collaboration_mode_kind: Option<ModeKind>,
) -> RolloutItem {
    RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(ContextAnchorSavedEvent {
        anchor_id: anchor_id.to_string(),
        label: label.map(str::to_string),
        history_boundary: boundary,
        created_at,
        collaboration_mode_kind,
    }))
}

fn turn_context(mode: ModeKind) -> RolloutItem {
    RolloutItem::TurnContext(TurnContextItem {
        turn_id: None,
        cwd: AbsolutePathBuf::from_absolute_path(
            std::env::current_dir().expect("current dir should be available"),
        )
        .expect("current dir should be absolute"),
        workspace_roots: None,
        current_date: None,
        timezone: None,
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::DangerFullAccess,
        permission_profile: None,
        network: None,
        file_system_sandbox_policy: None,
        model: "test-model".to_string(),
        comp_hash: None,
        personality: None,
        collaboration_mode: Some(CollaborationMode {
            mode,
            settings: Settings {
                model: "test-model".to_string(),
                reasoning_effort: None,
                developer_instructions: None,
            },
        }),
        multi_agent_version: None,
        multi_agent_mode: None,
        realtime_active: None,
        effort: None,
        summary: Default::default(),
    })
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
            response_items_reclaimed: 2,
            approx_tokens_reclaimed: 10,
            reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
            reclaim_threshold_tokens: Some(2_000),
            reclaim_threshold_met: Some(false),
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

fn function_call(call_id: &str) -> ResponseItem {
    ResponseItem::FunctionCall {
        id: None,
        name: "rewind_context_to_anchor".to_string(),
        namespace: None,
        arguments: "{}".to_string(),
        call_id: call_id.to_string(),
        internal_chat_message_metadata_passthrough: None,
    }
}

#[test]
fn count_user_turns_since_anchor_forgets_pre_compaction_anchor() {
    let items = vec![
        saved_anchor(
            ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0,
        ),
        user_message(),
        compaction(),
    ];

    let result = count_user_turns_since_anchor(&items, ANCHOR_ID);

    assert!(matches!(result, Err(CodexErr::InvalidRequest(_))));
}

#[test]
fn count_user_turns_since_anchor_uses_post_compaction_anchor() {
    let items = vec![
        saved_anchor(
            ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0,
        ),
        user_message(),
        compaction(),
        saved_anchor(
            ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0,
        ),
        user_message(),
    ];

    let result = count_user_turns_since_anchor(&items, ANCHOR_ID);

    assert_eq!(result.unwrap(), 1);
}

#[test]
fn list_context_anchors_returns_active_anchors_newest_first() {
    let rollout_items = vec![
        saved_anchor(
            "a",
            Some("early"),
            /*boundary*/ 1,
            /*created_at*/ 10,
        ),
        user_message(),
        saved_anchor(
            "b",
            Some("late"),
            /*boundary*/ 3,
            /*created_at*/ 20,
        ),
        user_message(),
    ];
    let current_history = vec![
        history_item("zero"),
        history_item("one"),
        history_item("two"),
        history_item("three"),
    ];

    let result = list_context_anchors_from_rollout(
        &rollout_items,
        &current_history,
        /*limit*/ 20,
        ModeKind::Default,
    );

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
    assert_eq!(
        result.anchors[0].collaboration_mode_kind,
        Some(ModeKind::Default)
    );
    assert_eq!(result.anchors[0].compatible_with_current_mode, Some(true));
    assert!(result.anchors[0].approx_tokens_since_anchor > 0);
    assert_eq!(result.anchors[1].anchor_id, "a");
    assert_eq!(result.anchors[1].response_items_since_anchor, 3);
    assert_eq!(result.anchors[1].user_turns_since_anchor, 2);
}

#[test]
fn list_context_anchors_omits_pre_compaction_anchors() {
    let rollout_items = vec![
        saved_anchor(
            "old", /*label*/ None, /*boundary*/ 0, /*created_at*/ 10,
        ),
        user_message(),
        compaction(),
        saved_anchor(
            "new", /*label*/ None, /*boundary*/ 1, /*created_at*/ 20,
        ),
    ];
    let current_history = vec![history_item("zero"), history_item("one")];

    let result = list_context_anchors_from_rollout(
        &rollout_items,
        &current_history,
        /*limit*/ 20,
        ModeKind::Default,
    );

    assert_eq!(result.active_anchor_count, 1);
    assert_eq!(result.invalidated_anchor_count, 1);
    assert_eq!(result.anchors.len(), 1);
    assert_eq!(result.anchors[0].anchor_id, "new");
}

#[test]
fn list_context_anchors_omits_anchors_after_rewound_target() {
    let rollout_items = vec![
        saved_anchor(
            "a", /*label*/ None, /*boundary*/ 0, /*created_at*/ 10,
        ),
        user_message(),
        saved_anchor(
            "b", /*label*/ None, /*boundary*/ 1, /*created_at*/ 20,
        ),
        user_message(),
        saved_anchor(
            "c", /*label*/ None, /*boundary*/ 2, /*created_at*/ 30,
        ),
        rewind("b"),
        user_message(),
    ];
    let current_history = vec![
        history_item("zero"),
        history_item("one"),
        history_item("carry"),
    ];

    let result = list_context_anchors_from_rollout(
        &rollout_items,
        &current_history,
        /*limit*/ 20,
        ModeKind::Default,
    );

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

#[test]
fn legacy_anchor_mode_is_inferred_from_latest_turn_context() {
    let rollout_items = vec![
        turn_context(ModeKind::Plan),
        saved_anchor_with_mode(
            ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0,
            /*collaboration_mode_kind*/ None,
        ),
    ];

    let anchor = latest_active_anchor_event(&rollout_items, ANCHOR_ID).unwrap();

    assert_eq!(anchor.collaboration_mode_kind, Some(ModeKind::Plan));
}

#[test]
fn legacy_anchor_without_turn_context_keeps_unknown_mode() {
    let rollout_items = vec![saved_anchor_with_mode(
        ANCHOR_ID, /*label*/ None, /*boundary*/ 0, /*created_at*/ 0,
        /*collaboration_mode_kind*/ None,
    )];

    let anchor = latest_active_anchor_event(&rollout_items, ANCHOR_ID).unwrap();

    assert_eq!(anchor.collaboration_mode_kind, None);
}

#[test]
fn rewind_benefit_counts_items_after_anchor_and_excludes_current_call() {
    let anchor = ContextAnchorSavedEvent {
        anchor_id: ANCHOR_ID.to_string(),
        label: None,
        history_boundary: 1,
        created_at: 0,
        collaboration_mode_kind: Some(ModeKind::Default),
    };
    let current_history = vec![
        history_item("before anchor"),
        history_item("reclaimed one"),
        function_call("current-rewind"),
        history_item("reclaimed two"),
    ];

    let benefit = rewind_benefit_since_anchor(
        &anchor,
        &current_history,
        "current-rewind",
        Some(/*model_context_window*/ 10_000),
    );

    assert_eq!(benefit.response_items_reclaimed, 2);
    assert!(benefit.approx_tokens_reclaimed > 0);
    assert_eq!(
        benefit.reclaim_threshold_percent,
        CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT
    );
    assert_eq!(benefit.reclaim_threshold_tokens, Some(2_000));
    assert_eq!(benefit.reclaim_threshold_met, Some(false));
}

#[test]
fn rewind_benefit_is_zero_for_near_anchor() {
    let anchor = ContextAnchorSavedEvent {
        anchor_id: ANCHOR_ID.to_string(),
        label: None,
        history_boundary: 1,
        created_at: 0,
        collaboration_mode_kind: Some(ModeKind::Default),
    };
    let current_history = vec![
        history_item("before anchor"),
        function_call("current-rewind"),
    ];

    let benefit = rewind_benefit_since_anchor(
        &anchor,
        &current_history,
        "current-rewind",
        Some(/*model_context_window*/ 10_000),
    );

    assert_eq!(
        benefit,
        ContextRewindBenefit {
            response_items_reclaimed: 0,
            approx_tokens_reclaimed: 0,
            reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
            reclaim_threshold_tokens: Some(2_000),
            reclaim_threshold_met: Some(false),
        }
    );
}

#[test]
fn rewind_benefit_omits_threshold_result_without_context_window() {
    let anchor = ContextAnchorSavedEvent {
        anchor_id: ANCHOR_ID.to_string(),
        label: None,
        history_boundary: 1,
        created_at: 0,
        collaboration_mode_kind: Some(ModeKind::Default),
    };
    let current_history = vec![
        history_item("before anchor"),
        history_item("reclaimed one"),
        function_call("current-rewind"),
    ];

    let benefit = rewind_benefit_since_anchor(
        &anchor,
        &current_history,
        "current-rewind",
        /*model_context_window*/ None,
    );

    assert_eq!(
        benefit.reclaim_threshold_percent,
        CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT
    );
    assert_eq!(benefit.reclaim_threshold_tokens, None);
    assert_eq!(benefit.reclaim_threshold_met, None);
}

#[test]
fn min_reclaim_percent_allows_rewind_when_disabled() {
    let benefit = ContextRewindBenefit {
        response_items_reclaimed: 0,
        approx_tokens_reclaimed: 0,
        reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
        reclaim_threshold_tokens: Some(20),
        reclaim_threshold_met: Some(false),
    };

    let result = evaluate_min_reclaim_percent(
        &benefit,
        Some(/*model_context_window*/ 100),
        /*min_reclaim_percent*/ 0,
    );

    assert_eq!(result.unwrap(), None);
}

#[test]
fn min_reclaim_percent_marks_rewind_below_threshold_rejected() {
    let benefit = ContextRewindBenefit {
        response_items_reclaimed: 1,
        approx_tokens_reclaimed: 19,
        reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
        reclaim_threshold_tokens: Some(20),
        reclaim_threshold_met: Some(false),
    };

    let result = evaluate_min_reclaim_percent(
        &benefit,
        Some(/*model_context_window*/ 100),
        /*min_reclaim_percent*/ 20,
    );

    assert_eq!(
        result.unwrap(),
        Some((
            ContextRewindRejectionReason::BelowMinReclaimPercent,
            Some(20),
            Some(100)
        ))
    );
}

#[test]
fn min_reclaim_percent_allows_rewind_at_threshold() {
    let benefit = ContextRewindBenefit {
        response_items_reclaimed: 1,
        approx_tokens_reclaimed: 20,
        reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
        reclaim_threshold_tokens: Some(20),
        reclaim_threshold_met: Some(true),
    };

    let result = evaluate_min_reclaim_percent(
        &benefit,
        Some(/*model_context_window*/ 100),
        /*min_reclaim_percent*/ 20,
    );

    assert_eq!(result.unwrap(), None);
}

#[test]
fn min_reclaim_percent_marks_unknown_context_window_rejected() {
    let benefit = ContextRewindBenefit {
        response_items_reclaimed: 1,
        approx_tokens_reclaimed: 20,
        reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
        reclaim_threshold_tokens: None,
        reclaim_threshold_met: None,
    };

    let result = evaluate_min_reclaim_percent(
        &benefit, /*model_context_window*/ None, /*min_reclaim_percent*/ 20,
    );

    assert_eq!(
        result.unwrap(),
        Some((
            ContextRewindRejectionReason::UnknownContextWindowForMinReclaimPercent,
            None,
            None
        ))
    );
}

#[test]
fn collaboration_mode_guard_allows_same_mode() {
    let result =
        validate_anchor_collaboration_mode(ANCHOR_ID, Some(ModeKind::Default), ModeKind::Default);

    assert!(result.is_ok());
}

#[test]
fn collaboration_mode_guard_allows_unknown_legacy_anchor_mode() {
    let result = validate_anchor_collaboration_mode(
        ANCHOR_ID,
        /*anchor_collaboration_mode_kind*/ None,
        ModeKind::Default,
    );

    assert!(result.is_ok());
}

#[test]
fn collaboration_mode_guard_rejects_cross_mode_rewind() {
    let result =
        validate_anchor_collaboration_mode(ANCHOR_ID, Some(ModeKind::Plan), ModeKind::Default);

    assert_eq!(
        result.unwrap_err().to_string(),
        "context rewind to anchor `anchor` rejected: anchor was saved in Plan mode, but current mode is Default"
    );
}

#[test]
fn context_rewound_to_anchor_event_defaults_reclaim_fields() {
    let event: ContextRewoundToAnchorEvent = serde_json::from_value(serde_json::json!({
        "anchor_id": ANCHOR_ID,
        "dropped_turns": 1,
        "note": "carry",
    }))
    .expect("legacy context rewind event should deserialize");

    assert_eq!(event.response_items_reclaimed, 0);
    assert_eq!(event.approx_tokens_reclaimed, 0);
    assert_eq!(
        event.reclaim_threshold_percent,
        CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT
    );
    assert_eq!(event.reclaim_threshold_tokens, None);
    assert_eq!(event.reclaim_threshold_met, None);
}
