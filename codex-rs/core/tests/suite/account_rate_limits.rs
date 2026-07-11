use anyhow::Result;
use codex_login::CodexAuth;
use codex_protocol::openai_models::TruncationPolicyConfig;
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
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

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
            "rate_limits": [],
        })
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_rate_limits_output_uses_model_truncation_policy() -> Result<()> {
    let server = start_mock_server().await;
    let additional_rate_limits = (0..12)
        .map(|index| {
            json!({
                "limit_name": format!("Feature limit {index}"),
                "metered_feature": format!("codex_feature_{index}"),
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": index,
                        "limit_window_seconds": 3600,
                        "reset_after_seconds": 120,
                        "reset_at": 1735689720 + index,
                    },
                    "secondary_window": {
                        "used_percent": index + 10,
                        "limit_window_seconds": 86400,
                        "reset_after_seconds": 43200,
                        "reset_at": 1735693200 + index,
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "plan_type": "pro",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 42,
                    "limit_window_seconds": 3600,
                    "reset_after_seconds": 120,
                    "reset_at": 1735689720,
                }
            },
            "additional_rate_limits": additional_rate_limits,
        })))
        .expect(1)
        .mount(&server)
        .await;
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
    let chatgpt_base_url = server.uri();
    let test = test_codex()
        .with_auth(CodexAuth::create_dummy_chatgpt_auth_for_testing())
        .with_model_info_override("gpt-5.4", |model_info| {
            model_info.truncation_policy = TruncationPolicyConfig::tokens(40);
        })
        .with_config(move |config| {
            config.chatgpt_base_url = chatgpt_base_url;
            config.tool_output_token_limit = Some(40);
        })
        .build(&server)
        .await?;

    test.submit_turn("check account rate limits").await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    let (content, success) = requests[1]
        .function_call_output_content_and_success(CALL_ID)
        .expect("account rate limits tool output should be present");
    let content = content.expect("account rate limits tool output should contain text");
    assert_eq!(success, None);
    assert!(
        content.contains("tokens truncated"),
        "account rate limits output should use the model truncation policy: {content}"
    );
    assert!(!content.contains("total_rate_limit_count"));
    assert!(!content.contains("\"truncated\":"));

    Ok(())
}
