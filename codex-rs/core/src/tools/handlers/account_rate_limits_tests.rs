use super::*;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context;
use crate::tools::TELEMETRY_PREVIEW_MAX_BYTES;
use crate::tools::TELEMETRY_PREVIEW_TRUNCATION_NOTICE;
use crate::tools::context::ToolCallSource;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::CreditsSnapshot;
use codex_protocol::protocol::RateLimitSnapshot;
use codex_protocol::protocol::SpendControlLimitSnapshot;
use core_test_support::responses::start_mock_server;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const CALL_ID: &str = "call-1";
const CUSTOM_PROVIDER_ID: &str = "custom";

fn invocation(
    session: crate::session::session::Session,
    turn: crate::session::turn_context::TurnContext,
) -> ToolInvocation {
    let turn = Arc::new(turn);
    ToolInvocation {
        session: Arc::new(session),
        step_context: StepContext::for_test(Arc::clone(&turn)),
        turn,
        cancellation_token: tokio_util::sync::CancellationToken::new(),
        tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
        call_id: CALL_ID.to_string(),
        tool_name: ToolName::plain(GET_ACCOUNT_RATE_LIMITS_TOOL_NAME),
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
    } = output.to_response_item(CALL_ID, payload)
    else {
        panic!("expected function call output");
    };
    let content = function_output
        .body
        .to_text()
        .expect("account rate limit output should be text");
    serde_json::from_str(&content).expect("account rate limit output should be JSON")
}

#[tokio::test]
async fn reports_structured_unavailable_reasons() {
    let expected = [
        (None, OPENAI_PROVIDER_ID, "not_logged_in"),
        (
            Some(CodexAuth::from_api_key("test-key")),
            OPENAI_PROVIDER_ID,
            "api_key_auth",
        ),
        (
            Some(CodexAuth::create_dummy_chatgpt_auth_for_testing()),
            CUSTOM_PROVIDER_ID,
            "custom_provider",
        ),
    ];

    for (auth, provider_id, unavailable_reason) in expected {
        let (session, mut turn) = make_session_and_context().await;
        turn.auth_manager = auth.map(AuthManager::from_auth_for_testing);
        let mut config = (*turn.config).clone();
        config.model_provider_id = provider_id.to_string();
        turn.config = Arc::new(config);
        let invocation = invocation(session, turn);
        let payload = invocation.payload.clone();
        let output = AccountRateLimitsHandler
            .handle(invocation)
            .await
            .expect("unavailable state should be returned successfully");

        assert_eq!(
            output_json(output.as_ref(), &payload),
            json!({
                "available": false,
                "unavailable_reason": unavailable_reason,
                "rate_limits": [],
            })
        );
    }
}

#[tokio::test]
async fn fetches_all_rate_limit_buckets_and_derives_remaining_percent() {
    let server = start_mock_server().await;
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
                },
                "secondary_window": {
                    "used_percent": 125,
                    "limit_window_seconds": 86400,
                    "reset_after_seconds": 43200,
                    "reset_at": 1735693200,
                }
            },
            "additional_rate_limits": [{
                "limit_name": "Other limit",
                "metered_feature": "codex_other",
                "rate_limit": {
                    "allowed": true,
                    "limit_reached": false,
                    "primary_window": {
                        "used_percent": 5,
                        "limit_window_seconds": 1800,
                        "reset_after_seconds": 600,
                        "reset_at": 1735693200,
                    }
                }
            }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (session, mut turn) = make_session_and_context().await;
    turn.auth_manager = Some(AuthManager::from_auth_for_testing(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
    ));
    let mut config = (*turn.config).clone();
    config.chatgpt_base_url = server.uri();
    turn.config = Arc::new(config);

    let invocation = invocation(session, turn);
    let payload = invocation.payload.clone();
    let output = AccountRateLimitsHandler
        .handle(invocation)
        .await
        .expect("account rate limit fetch should succeed");
    let expected = json!({
        "available": true,
        "unavailable_reason": null,
        "rate_limits": [
            {
                "limit_id": "codex",
                "limit_name": null,
                "primary": {
                    "used_percent": 42.0,
                    "remaining_percent": 58.0,
                    "window_minutes": 60,
                    "resets_at": 1735689720,
                },
                "secondary": {
                    "used_percent": 125.0,
                    "remaining_percent": 0.0,
                    "window_minutes": 1440,
                    "resets_at": 1735693200,
                },
                "credits": null,
                "individual_limit": null,
                "plan_type": "pro",
                "rate_limit_reached_type": null,
            },
            {
                "limit_id": "codex_other",
                "limit_name": "Other limit",
                "primary": {
                    "used_percent": 5.0,
                    "remaining_percent": 95.0,
                    "window_minutes": 30,
                    "resets_at": 1735693200,
                },
                "secondary": null,
                "credits": null,
                "individual_limit": null,
                "plan_type": "pro",
                "rate_limit_reached_type": null,
            }
        ]
    });

    assert_eq!(output_json(output.as_ref(), &payload), expected);
    assert_eq!(output.code_mode_result(&payload), expected);
}

#[tokio::test]
async fn backend_decode_failure_is_a_failed_tool_call() {
    const BACKEND_BODY: &str = "backend-body-must-not-reach-model";
    let server = start_mock_server().await;
    Mock::given(method("GET"))
        .and(path("/api/codex/usage"))
        .respond_with(ResponseTemplate::new(200).set_body_string(BACKEND_BODY))
        .expect(1)
        .mount(&server)
        .await;

    let (session, mut turn) = make_session_and_context().await;
    turn.auth_manager = Some(AuthManager::from_auth_for_testing(
        CodexAuth::create_dummy_chatgpt_auth_for_testing(),
    ));
    let mut config = (*turn.config).clone();
    config.chatgpt_base_url = server.uri();
    turn.config = Arc::new(config);

    let error = match AccountRateLimitsHandler
        .handle(invocation(session, turn))
        .await
    {
        Ok(_) => panic!("invalid backend response should fail the tool call"),
        Err(error) => error,
    };

    let FunctionCallError::RespondToModel(message) = error else {
        panic!("backend decode failure should be reported to the model");
    };
    assert_eq!(message, FETCH_ERROR_MESSAGE);
}

#[test]
fn preserves_all_buckets_and_backend_strings() {
    let long_value = "é".repeat(8 * 1024);
    let bucket_count = 6;
    let snapshots = (0..bucket_count)
        .map(|_| RateLimitSnapshot {
            limit_id: Some(long_value.clone()),
            limit_name: Some(long_value.clone()),
            primary: None,
            secondary: None,
            credits: Some(CreditsSnapshot {
                has_credits: true,
                unlimited: false,
                balance: Some(long_value.clone()),
            }),
            individual_limit: Some(SpendControlLimitSnapshot {
                limit: long_value.clone(),
                used: long_value.clone(),
                remaining_percent: 50,
                resets_at: 1,
            }),
            plan_type: None,
            rate_limit_reached_type: None,
        })
        .collect();

    let response = AccountRateLimitsResponse::available(snapshots);
    let output = AccountRateLimitsOutput::new(response)
        .expect("account rate limits should serialize without truncation");
    let payload = ToolPayload::Function {
        arguments: "{}".to_string(),
    };
    let expected_bucket = json!({
        "limit_id": long_value.clone(),
        "limit_name": long_value.clone(),
        "primary": null,
        "secondary": null,
        "credits": {
            "has_credits": true,
            "unlimited": false,
            "balance": long_value.clone(),
        },
        "individual_limit": {
            "limit": long_value.clone(),
            "used": long_value,
            "remaining_percent": 50,
            "resets_at": 1,
        },
        "plan_type": null,
        "rate_limit_reached_type": null,
    });

    let expected = json!({
        "available": true,
        "unavailable_reason": null,
        "rate_limits": vec![expected_bucket; bucket_count],
    });
    let function_output = output_json(&output, &payload);
    let code_mode_result = output.code_mode_result(&payload);
    let log_preview = output.log_preview();

    assert_eq!(function_output, expected);
    assert_eq!(code_mode_result, expected);
    assert_eq!(function_output, code_mode_result);
    assert!(
        log_preview.len()
            <= TELEMETRY_PREVIEW_MAX_BYTES + TELEMETRY_PREVIEW_TRUNCATION_NOTICE.len() + 1
    );
    assert!(log_preview.ends_with(TELEMETRY_PREVIEW_TRUNCATION_NOTICE));
}
