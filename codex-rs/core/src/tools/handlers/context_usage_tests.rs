use super::*;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::TokenUsage;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

fn invocation(
    session: crate::session::session::Session,
    turn: crate::session::turn_context::TurnContext,
) -> ToolInvocation {
    let turn = Arc::new(turn);
    let step_context = StepContext::for_test(Arc::clone(&turn));
    ToolInvocation {
        session: Arc::new(session),
        step_context,
        turn,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
        call_id: "call-1".to_string(),
        tool_name: codex_tools::ToolName::plain(GET_CONTEXT_USAGE_TOOL_NAME),
        source: ToolCallSource::Direct,
        payload: ToolPayload::Function {
            arguments: "{}".to_string(),
        },
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
        .expect("context usage output should be text");
    serde_json::from_str(&content).expect("context usage output should be JSON")
}

#[tokio::test]
async fn get_context_usage_reports_current_context_usage_snapshot() {
    let (session, mut turn) = make_session_and_context().await;
    turn.model_info.context_window = Some(100_000);
    turn.model_info.effective_context_window_percent = 100;

    session
        .record_token_usage_info(
            &turn,
            Some(&TokenUsage {
                total_tokens: 34_000,
                ..TokenUsage::default()
            }),
        )
        .await
        .expect("record token usage");

    let invocation = invocation(session, turn);
    let payload = invocation.payload.clone();
    let output = ContextUsageHandler
        .handle(invocation)
        .await
        .expect("context usage should succeed");

    assert_eq!(
        output_json(output.as_ref(), &payload),
        json!({
            "usage_known": true,
            "model_context_window": 100000,
            "used_tokens": 34000,
            "remaining_tokens": 66000,
            "remaining_percent": 75,
            "source": "token_usage_info",
        })
    );
    assert_eq!(
        output.code_mode_result(&payload),
        json!({
            "usage_known": true,
            "model_context_window": 100000,
            "used_tokens": 34000,
            "remaining_tokens": 66000,
            "remaining_percent": 75,
            "source": "token_usage_info",
        })
    );
}

#[tokio::test]
async fn get_context_usage_reports_unknown_usage_without_estimating() {
    let (session, mut turn) = make_session_and_context().await;
    turn.model_info.context_window = Some(100_000);
    turn.model_info.effective_context_window_percent = 100;

    let invocation = invocation(session, turn);
    let payload = invocation.payload.clone();
    let output = ContextUsageHandler
        .handle(invocation)
        .await
        .expect("context usage should succeed");

    assert_eq!(
        output_json(output.as_ref(), &payload),
        json!({
            "usage_known": false,
            "model_context_window": 100000,
            "used_tokens": null,
            "remaining_tokens": null,
            "remaining_percent": null,
            "source": "model_context_window_only",
        })
    );
}
