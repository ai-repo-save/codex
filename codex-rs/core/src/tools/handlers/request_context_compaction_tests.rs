use super::*;
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

async fn invocation(arguments: String) -> ToolInvocation {
    let (session, turn) = make_session_and_context().await;
    ToolInvocation {
        session: Arc::new(session),
        turn: Arc::new(turn),
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
        call_id: "call-1".to_string(),
        tool_name: codex_tools::ToolName::plain(REQUEST_CONTEXT_COMPACTION_TOOL_NAME),
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
        .expect("request context compaction output should be text");
    serde_json::from_str(&content).expect("request context compaction output should be JSON")
}

#[tokio::test]
async fn request_context_compaction_returns_note_as_compacted_tool_output() {
    let invocation = invocation(r#"{"note":" remember the next step "}"#.to_string()).await;
    let payload = invocation.payload.clone();
    let output = RequestContextCompactionHandler
        .handle(invocation)
        .await
        .expect("request context compaction should succeed");

    assert_eq!(
        output_json(output.as_ref(), &payload),
        json!({
            "compacted": true,
            "mode": "mid_turn",
            "note": "remember the next step",
            "note_bytes": 22,
        })
    );
    assert_eq!(
        output.code_mode_result(&payload),
        json!({
            "compacted": true,
            "mode": "mid_turn",
            "note": "remember the next step",
            "note_bytes": 22,
        })
    );
}

#[tokio::test]
async fn request_context_compaction_rejects_empty_note() {
    let result = RequestContextCompactionHandler
        .handle(invocation(r#"{"note":"   "}"#.to_string()).await)
        .await;
    let Err(err) = result else {
        panic!("empty note should fail");
    };

    assert_eq!(err.to_string(), "`note` must be a non-empty string");
}

#[tokio::test]
async fn request_context_compaction_rejects_oversized_note() {
    let note = "x".repeat(MAX_CONTEXT_COMPACTION_NOTE_BYTES + 1);
    let result = RequestContextCompactionHandler
        .handle(invocation(json!({ "note": note }).to_string()).await)
        .await;
    let Err(err) = result else {
        panic!("oversized note should fail");
    };

    assert_eq!(
        err.to_string(),
        "`note` is 16385 bytes, but the maximum is 16384 bytes"
    );
}
