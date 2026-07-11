use anyhow::Result;
use codex_features::Feature;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::Respond;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const MULTI_AGENT_V2_NAMESPACE: &str = "agents";
const ROOT_PROMPT: &str = "spawn the worker and wait for its question";
const CHILD_PROMPT: &str = "ask the parent for the authoritative decision";
const QUESTION: &str = "which release channel is authoritative?";
const ANSWER: &str = "stable is authoritative";
const SPAWN_CALL_ID: &str = "spawn-worker";
const WAIT_CALL_ID: &str = "wait-for-worker";
const ASK_PARENT_CALL_ID: &str = "ask-parent";
const REPLY_CALL_ID: &str = "reply-to-child";

#[derive(Debug, Default)]
struct AskParentResponder {
    root_started: AtomicBool,
    child_started: AtomicBool,
}

impl Respond for AskParentResponder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body = request_body(request);

        if contains_text(&body, ROOT_PROMPT) && !self.root_started.swap(true, Ordering::SeqCst) {
            let args = json!({
                "message": CHILD_PROMPT,
                "task_name": "worker",
            });
            return tool_call_response("root-spawn-response", SPAWN_CALL_ID, "spawn_agent", args);
        }

        if contains_text(&body, CHILD_PROMPT) && !self.child_started.swap(true, Ordering::SeqCst) {
            return tool_call_response(
                "child-question-response",
                ASK_PARENT_CALL_ID,
                "ask_parent",
                json!({"question": QUESTION}),
            );
        }

        if has_call_output(&body, ASK_PARENT_CALL_ID) {
            return sse_response(sse(vec![
                ev_response_created("child-finished-response"),
                ev_assistant_message("child-finished-message", "child finished"),
                ev_completed("child-finished-response"),
            ]));
        }

        if has_call_output(&body, REPLY_CALL_ID) {
            return sse_response(sse(vec![
                ev_response_created("root-finished-response"),
                ev_assistant_message("root-finished-message", "root finished"),
                ev_completed("root-finished-response"),
            ]));
        }

        if let Some(request_id) = parent_request_id(&body) {
            return tool_call_response(
                "root-reply-response",
                REPLY_CALL_ID,
                "send_message",
                json!({
                    "target": "/root/worker",
                    "message": ANSWER,
                    "in_reply_to": request_id,
                }),
            );
        }

        if has_call_output(&body, SPAWN_CALL_ID) {
            return tool_call_response("root-wait-response", WAIT_CALL_ID, "wait_agent", json!({}));
        }

        sse_response(sse(vec![
            ev_response_created("root-finished-response"),
            ev_assistant_message("root-finished-message", "root finished"),
            ev_completed("root-finished-response"),
        ]))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_question_reaches_active_parent_and_correlated_reply_unblocks_child() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(AskParentResponder::default())
        .mount(&server)
        .await;

    let test = test_codex()
        .with_model("koffing")
        .with_config(|config| {
            config
                .features
                .enable(Feature::Collab)
                .expect("test config should allow feature update");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("test config should allow feature update");
            config.model_provider.supports_websockets = false;
        })
        .build(&server)
        .await?;

    test.submit_turn(ROOT_PROMPT).await?;

    let requests = server.received_requests().await.unwrap_or_default();
    let ask_parent_output = requests
        .iter()
        .map(request_body)
        .find_map(|body| call_output_text(&body, ASK_PARENT_CALL_ID))
        .expect("child should receive ask_parent output");
    let ask_parent_result: Value = serde_json::from_str(&ask_parent_output)?;
    assert_eq!(
        ask_parent_result.get("status"),
        Some(&Value::String("answered".to_string()))
    );
    assert_eq!(
        ask_parent_result.get("answer"),
        Some(&Value::String(ANSWER.to_string()))
    );

    let parent_request = requests
        .iter()
        .map(request_body)
        .find(|body| parent_request_id(body).is_some())
        .expect("active parent should receive the child request");
    assert!(has_call_output(&parent_request, WAIT_CALL_ID));

    Ok(())
}

fn tool_call_response(
    response_id: &str,
    call_id: &str,
    tool_name: &str,
    args: Value,
) -> ResponseTemplate {
    let args = serde_json::to_string(&args).expect("tool arguments should serialize");
    sse_response(sse(vec![
        ev_response_created(response_id),
        ev_function_call_with_namespace(call_id, MULTI_AGENT_V2_NAMESPACE, tool_name, &args),
        ev_completed(response_id),
    ]))
}

fn request_body(request: &wiremock::Request) -> Value {
    let bytes = if request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("zstd"))
    {
        zstd::stream::decode_all(std::io::Cursor::new(&request.body)).unwrap_or_default()
    } else {
        request.body.clone()
    };
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn contains_text(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(text) => text.contains(expected),
        Value::Array(items) => items.iter().any(|item| contains_text(item, expected)),
        Value::Object(fields) => fields.values().any(|item| contains_text(item, expected)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn has_call_output(body: &Value, call_id: &str) -> bool {
    body.get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call_output")
                    && item.get("call_id").and_then(Value::as_str) == Some(call_id)
            })
        })
}

fn call_output_text(body: &Value, call_id: &str) -> Option<String> {
    body.get("input")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some(call_id)
        })?
        .get("output")?
        .as_str()
        .map(str::to_string)
}

fn parent_request_id(body: &Value) -> Option<String> {
    let text = body
        .get("input")
        .and_then(Value::as_array)?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("agent_message"))
        .find_map(|item| {
            item.get("content")?
                .as_array()?
                .iter()
                .find_map(|content| content.get("text").and_then(Value::as_str))
        })?;
    text.strip_prefix("Parent decision request `")
        .and_then(|remainder| remainder.split('`').next())
        .map(str::to_string)
}
