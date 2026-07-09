use super::*;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_protocol::models::ResponseInputItem;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

async fn invocation(tool_name: &str, arguments: String) -> ToolInvocation {
    let (session, turn) = make_session_and_context().await;
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    ToolInvocation {
        session: Arc::new(session),
        step_context,
        turn,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
        call_id: "call-1".to_string(),
        tool_name: codex_tools::ToolName::plain(tool_name),
        source: ToolCallSource::Direct,
        payload: ToolPayload::Function { arguments },
    }
}

fn output_json(output: &dyn ToolOutput, payload: &ToolPayload) -> Value {
    let ResponseInputItem::FunctionCallOutput {
        output: function_output,
        ..
    } = output.to_response_item("call-1", payload)
    else {
        panic!("expected function call output");
    };
    let content = function_output
        .body
        .to_text()
        .expect("context anchor output should be text");
    serde_json::from_str(&content).expect("context anchor output should be JSON")
}

#[tokio::test]
async fn save_context_anchor_trims_label_and_returns_anchor_id() {
    let invocation = invocation(
        SAVE_CONTEXT_ANCHOR_TOOL_NAME,
        r#"{"label":" before branch "}"#.to_string(),
    )
    .await;
    let payload = invocation.payload.clone();
    let output = SaveContextAnchorHandler
        .handle(invocation)
        .await
        .expect("save context anchor should succeed");
    let json = output_json(output.as_ref(), &payload);

    assert_eq!(json["label"], json!("before branch"));
    assert!(
        json["anchor_id"]
            .as_str()
            .is_some_and(|anchor_id| anchor_id.starts_with("ctx-"))
    );
    assert!(json["created_at"].as_i64().is_some_and(|value| value > 0));
}

#[tokio::test]
async fn save_context_anchor_rejects_oversized_label() {
    let label = "x".repeat(MAX_CONTEXT_ANCHOR_LABEL_BYTES + 1);
    let result = SaveContextAnchorHandler
        .handle(
            invocation(
                SAVE_CONTEXT_ANCHOR_TOOL_NAME,
                json!({ "label": label }).to_string(),
            )
            .await,
        )
        .await;
    let Err(err) = result else {
        panic!("oversized label should fail");
    };

    assert_eq!(
        err.to_string(),
        "`label` is 257 bytes, but the maximum is 256 bytes"
    );
}

#[tokio::test]
async fn rewind_context_to_anchor_returns_validated_request() {
    let invocation = invocation(
        REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
        r#"{"anchor_id":" anchor-1 ","note":" carry this "}"#.to_string(),
    )
    .await;
    let payload = invocation.payload.clone();
    let output = RewindContextToAnchorHandler
        .handle(invocation)
        .await
        .expect("rewind context to anchor should validate");

    assert_eq!(
        output_json(output.as_ref(), &payload),
        json!({
            "anchor_id": "anchor-1",
            "note": "carry this",
        })
    );
}

#[tokio::test]
async fn rewind_context_to_anchor_rejects_oversized_note() {
    let note = "x".repeat(MAX_CONTEXT_REWIND_NOTE_BYTES + 1);
    let result = RewindContextToAnchorHandler
        .handle(
            invocation(
                REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
                json!({ "anchor_id": "anchor-1", "note": note }).to_string(),
            )
            .await,
        )
        .await;
    let Err(err) = result else {
        panic!("oversized note should fail");
    };

    assert_eq!(
        err.to_string(),
        "`note` is 8193 bytes, but the maximum is 8192 bytes"
    );
}

#[test]
fn rewind_context_to_anchor_response_serializes_rewound_status() {
    let response = RewindContextToAnchorResponse::Rewound {
        anchor_id: "anchor-1".to_string(),
        replacement_anchor_id: "anchor-2".to_string(),
        dropped_turns: 2,
        response_items_reclaimed: 3,
        approx_tokens_reclaimed: 4,
        reclaim_threshold_percent: 20,
        reclaim_threshold_tokens: Some(100),
        reclaim_threshold_met: Some(false),
    };

    assert_eq!(
        serde_json::to_value(response).expect("response should serialize"),
        json!({
            "status": "rewound",
            "anchor_id": "anchor-1",
            "replacement_anchor_id": "anchor-2",
            "dropped_turns": 2,
            "response_items_reclaimed": 3,
            "approx_tokens_reclaimed": 4,
            "reclaim_threshold_percent": 20,
            "reclaim_threshold_tokens": 100,
            "reclaim_threshold_met": false,
        })
    );
}

#[test]
fn rewind_context_to_anchor_response_serializes_rejected_status() {
    let response = RewindContextToAnchorResponse::Rejected {
        anchor_id: "anchor-1".to_string(),
        dropped_turns: 2,
        response_items_reclaimed: 3,
        approx_tokens_reclaimed: 4,
        reclaim_threshold_percent: 20,
        reclaim_threshold_tokens: Some(100),
        reclaim_threshold_met: Some(false),
        reason: RewindContextToAnchorRejectionReason::BelowMinReclaimPercent,
        min_reclaim_percent: 10,
        min_reclaim_threshold_tokens: Some(50),
        model_context_window: Some(500),
    };

    assert_eq!(
        serde_json::to_value(response).expect("response should serialize"),
        json!({
            "status": "rejected",
            "anchor_id": "anchor-1",
            "dropped_turns": 2,
            "response_items_reclaimed": 3,
            "approx_tokens_reclaimed": 4,
            "reclaim_threshold_percent": 20,
            "reclaim_threshold_tokens": 100,
            "reclaim_threshold_met": false,
            "reason": "below_min_reclaim_percent",
            "min_reclaim_percent": 10,
            "min_reclaim_threshold_tokens": 50,
            "model_context_window": 500,
        })
    );
}

#[tokio::test]
async fn list_context_anchors_defaults_limit() {
    let invocation = invocation(LIST_CONTEXT_ANCHORS_TOOL_NAME, "{}".to_string()).await;
    let payload = invocation.payload.clone();
    let output = ListContextAnchorsHandler
        .handle(invocation)
        .await
        .expect("list context anchors should validate");

    assert_eq!(
        output_json(output.as_ref(), &payload),
        json!({ "limit": DEFAULT_LIST_CONTEXT_ANCHORS_LIMIT })
    );
}

#[tokio::test]
async fn list_context_anchors_rejects_zero_limit() {
    let result = ListContextAnchorsHandler
        .handle(
            invocation(
                LIST_CONTEXT_ANCHORS_TOOL_NAME,
                json!({ "limit": 0 }).to_string(),
            )
            .await,
        )
        .await;
    let Err(err) = result else {
        panic!("zero limit should fail");
    };

    assert_eq!(err.to_string(), "`limit` must be greater than 0");
}

#[tokio::test]
async fn list_context_anchors_rejects_oversized_limit() {
    let result = ListContextAnchorsHandler
        .handle(
            invocation(
                LIST_CONTEXT_ANCHORS_TOOL_NAME,
                json!({ "limit": MAX_LIST_CONTEXT_ANCHORS_LIMIT + 1 }).to_string(),
            )
            .await,
        )
        .await;
    let Err(err) = result else {
        panic!("oversized limit should fail");
    };

    assert_eq!(err.to_string(), "`limit` is 101, but the maximum is 100");
}
