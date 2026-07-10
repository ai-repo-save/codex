use anyhow::Result;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

const CALL_ID: &str = "rate-limits-call";
const TOOL_NAME: &str = "get_account_rate_limits";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_rate_limits_tool_returns_structured_api_key_unavailability() -> Result<()> {
    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(CALL_ID, TOOL_NAME, "{}"),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-2", "done"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;
    let test = test_codex().build(&server).await?;

    test.submit_turn("check account rate limits").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let (content, success) = requests[1]
        .function_call_output_content_and_success(CALL_ID)
        .expect("account rate limits tool output should be present");
    let content = content.expect("account rate limits tool output should contain JSON");
    assert_eq!(success, None);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&content)?,
        json!({
            "available": false,
            "unavailable_reason": "api_key_auth",
            "total_rate_limit_count": 0,
            "truncated": false,
            "rate_limits": [],
        })
    );

    Ok(())
}
