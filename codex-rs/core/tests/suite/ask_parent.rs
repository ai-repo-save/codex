use anyhow::Context;
use anyhow::Result;
use codex_features::Feature;
use codex_protocol::items::ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE;
use codex_protocol::items::CollabAgentTool;
use codex_protocol::items::CollabAgentToolCallStatus;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::ENVIRONMENT_CONTEXT_OPEN_TAG;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::sse;
use core_test_support::responses::sse_response;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
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
const CONSULT_QUESTION: &str = "summarize the parent snapshot without deciding";
const CONSULT_ADVISORY: &str = "the snapshot records the stable release channel";
const AUTHORITATIVE_REQUIRED_ADVISORY: &str = "a live parent decision is required";
const ROOT_SNAPSHOT_MESSAGE: &str = "the parent snapshot records the stable release channel";
const CONSULT_CALL_ID: &str = "consult-parent";
const CONSULT_MESSAGE_CALL_ID: &str = "consult-send-message";
const CONSULT_LOCAL_TOOL_CALL_ID: &str = "consult-local-tool";
const CONSULT_LOCAL_COMMAND: &str = "touch consult-local-tool-must-not-run";
const CONSULT_LOCAL_FILENAME: &str = "consult-local-tool-must-not-run";
const CONSULT_MESSAGE_TARGET: &str = "/root/worker";
const CONSULT_UNDELIVERED_MESSAGE: &str = "consult must not message the worker";

#[derive(Clone, Copy, Debug)]
enum ConsultOutcome {
    Advisory,
    RequiresAuthoritativeParent,
    Invalid,
}

impl ConsultOutcome {
    fn kind(self) -> &'static str {
        match self {
            Self::Advisory => "advisory",
            Self::RequiresAuthoritativeParent => "requires_authoritative_parent",
            Self::Invalid => unreachable!("invalid consult responses have no kind"),
        }
    }

    fn advisory(self) -> &'static str {
        match self {
            Self::Advisory => CONSULT_ADVISORY,
            Self::RequiresAuthoritativeParent => AUTHORITATIVE_REQUIRED_ADVISORY,
            Self::Invalid => "invalid consult response",
        }
    }

    fn response_text(self) -> String {
        match self {
            Self::Invalid => self.advisory().to_string(),
            Self::Advisory | Self::RequiresAuthoritativeParent => json!({
                "kind": self.kind(),
                "advisory": self.advisory(),
            })
            .to_string(),
        }
    }
}

#[derive(Debug, Default)]
struct AskParentResponder {
    root_started: AtomicBool,
    child_started: AtomicBool,
}

#[derive(Debug)]
struct ConsultResponder {
    outcome: ConsultOutcome,
    request_local_tool: bool,
    consult_response_delay: Duration,
    root_started: AtomicBool,
    child_started: AtomicBool,
}

impl ConsultResponder {
    fn new(outcome: ConsultOutcome, request_local_tool: bool) -> Self {
        Self {
            outcome,
            request_local_tool,
            consult_response_delay: Duration::ZERO,
            root_started: AtomicBool::new(false),
            child_started: AtomicBool::new(false),
        }
    }

    fn with_consult_response_delay(mut self, delay: Duration) -> Self {
        self.consult_response_delay = delay;
        self
    }
}

impl Respond for ConsultResponder {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let body = request_body(request);

        if is_consult_responder_request(&body) {
            if self.request_local_tool && !has_call_output(&body, CONSULT_MESSAGE_CALL_ID) {
                return tool_call_response(
                    "consult-send-message-response",
                    CONSULT_MESSAGE_CALL_ID,
                    "send_message",
                    json!({
                        "target": CONSULT_MESSAGE_TARGET,
                        "message": CONSULT_UNDELIVERED_MESSAGE,
                    }),
                );
            }

            if self.request_local_tool && !has_call_output(&body, CONSULT_LOCAL_TOOL_CALL_ID) {
                let arguments = serde_json::to_string(&json!({ "command": CONSULT_LOCAL_COMMAND }))
                    .expect("consult local tool arguments should serialize");
                return sse_response(sse(vec![
                    ev_response_created("consult-local-tool-response"),
                    ev_function_call(CONSULT_LOCAL_TOOL_CALL_ID, "shell_command", &arguments),
                    ev_completed("consult-local-tool-response"),
                ]));
            }

            return final_message_response(
                "consult-responder-finished",
                &self.outcome.response_text(),
            )
            .set_delay(self.consult_response_delay);
        }

        if contains_text(&body, ROOT_PROMPT) && !self.root_started.swap(true, Ordering::SeqCst) {
            return tool_call_response(
                "consult-root-spawn-response",
                SPAWN_CALL_ID,
                "spawn_agent",
                json!({
                    "message": CHILD_PROMPT,
                    "task_name": "worker",
                }),
            );
        }

        if has_call_output(&body, SPAWN_CALL_ID) && !has_call_output(&body, WAIT_CALL_ID) {
            return tool_call_response(
                "consult-root-wait-response",
                WAIT_CALL_ID,
                "wait_agent",
                json!({}),
            );
        }

        if has_call_output(&body, WAIT_CALL_ID) {
            return final_message_response("consult-root-finished", ROOT_SNAPSHOT_MESSAGE);
        }

        if contains_text(&body, CHILD_PROMPT) && !self.child_started.swap(true, Ordering::SeqCst) {
            return tool_call_response(
                "consult-child-question-response",
                CONSULT_CALL_ID,
                "ask_parent",
                json!({
                    "question": CONSULT_QUESTION,
                    "mode": "consult",
                }),
            );
        }

        if has_call_output(&body, CONSULT_CALL_ID) {
            return final_message_response("consult-child-finished", "child finished");
        }

        final_message_response("consult-fallback-finished", "unexpected request")
    }
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

        if has_call_output(&body, SPAWN_CALL_ID) && !has_call_output(&body, WAIT_CALL_ID) {
            return tool_call_response("root-wait-response", WAIT_CALL_ID, "wait_agent", json!({}));
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

    let (requests, ask_parent_output) = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests = server.received_requests().await.unwrap_or_default();
            if let Some(output) = requests
                .iter()
                .map(request_body)
                .find_map(|body| call_output_text(&body, ASK_PARENT_CALL_ID))
            {
                break (requests, output);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .context("child should receive ask_parent output")?;
    let ask_parent_result: Value = serde_json::from_str(&ask_parent_output)
        .with_context(|| format!("ask_parent output was {ask_parent_output:?}"))?;
    assert_eq!(
        ask_parent_result.get("status"),
        Some(&Value::String("answered".to_string()))
    );
    assert_eq!(
        ask_parent_result.get("answer"),
        Some(&Value::String(ANSWER.to_string()))
    );
    assert_eq!(
        ask_parent_result.get("mode"),
        Some(&Value::String("authoritative".to_string()))
    );

    let parent_request = requests
        .iter()
        .map(request_body)
        .find(|body| parent_request_id(body).is_some())
        .expect("active parent should receive the child request");
    assert!(has_call_output(&parent_request, WAIT_CALL_ID));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consult_uses_a_fixed_parent_snapshot_without_waking_the_parent() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let (test, _server, requests, result) = run_consult(ConsultOutcome::Advisory, true).await?;

    assert_eq!(
        result.get("status"),
        Some(&Value::String("answered".to_string()))
    );
    assert_eq!(
        result.get("mode"),
        Some(&Value::String("consult".to_string()))
    );
    assert_eq!(
        result.get("advisory"),
        Some(&Value::String(CONSULT_ADVISORY.to_string()))
    );
    assert_eq!(result.get("answer"), Some(&Value::Null));
    assert_eq!(
        result.get("snapshot_may_be_stale"),
        Some(&Value::Bool(true))
    );
    assert!(
        result
            .get("snapshot_revision")
            .and_then(Value::as_str)
            .is_some_and(|revision| !revision.is_empty())
    );
    let parent_thread_id = test.session_configured.thread_id.to_string();
    assert_eq!(
        result.get("parent_thread_id").and_then(Value::as_str),
        Some(parent_thread_id.as_str())
    );

    let root_request = requests
        .iter()
        .find(|body| contains_text(body, ROOT_PROMPT) && !has_call_output(body, SPAWN_CALL_ID))
        .expect("parent request should be captured");
    let consult_request = requests
        .iter()
        .find(|body| {
            contains_text(body, CONSULT_QUESTION)
                && !has_call_output(body, CONSULT_LOCAL_TOOL_CALL_ID)
        })
        .expect("consult responder request should be captured");
    assert!(contains_text(consult_request, ROOT_PROMPT));
    assert!(!contains_text(consult_request, CONSULT_CALL_ID));
    assert_eq!(
        consult_request.get("instructions"),
        root_request.get("instructions")
    );
    assert_eq!(consult_request.get("model"), root_request.get("model"));
    assert_eq!(consult_request.get("tools"), root_request.get("tools"));
    assert_eq!(
        consult_request.get("tool_choice"),
        root_request.get("tool_choice")
    );
    assert_eq!(
        consult_request.get("prompt_cache_key"),
        root_request.get("prompt_cache_key")
    );
    assert_eq!(
        text_containing(consult_request, ENVIRONMENT_CONTEXT_OPEN_TAG),
        text_containing(root_request, ENVIRONMENT_CONTEXT_OPEN_TAG)
    );
    assert!(
        !requests
            .iter()
            .any(|body| parent_request_id(body).is_some())
    );
    assert!(
        requests
            .iter()
            .any(|body| has_call_output(body, CONSULT_LOCAL_TOOL_CALL_ID))
    );
    assert!(requests.iter().any(|body| {
        call_output_text(body, CONSULT_LOCAL_TOOL_CALL_ID).is_some_and(|output| !output.is_empty())
    }));
    assert!(requests.iter().any(|body| {
        call_output_text(body, CONSULT_MESSAGE_CALL_ID).is_some_and(|output| !output.is_empty())
    }));
    assert!(
        !requests
            .iter()
            .any(|body| contains_text(body, CONSULT_UNDELIVERED_MESSAGE)
                && !is_consult_responder_request(body))
    );
    assert!(!test.workspace_path(CONSULT_LOCAL_FILENAME).exists());
    assert_eq!(test.thread_manager.list_thread_ids().await.len(), 2);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consult_requires_authoritative_parent_without_automatic_escalation() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let (_test, _server, requests, result) =
        run_consult(ConsultOutcome::RequiresAuthoritativeParent, false).await?;

    assert_eq!(
        result.get("status"),
        Some(&Value::String(
            ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE.to_string()
        ))
    );
    assert_eq!(
        result.get("advisory"),
        Some(&Value::String(AUTHORITATIVE_REQUIRED_ADVISORY.to_string()))
    );
    assert!(
        !requests
            .iter()
            .any(|body| parent_request_id(body).is_some())
    );
    assert!(
        !requests
            .iter()
            .any(|body| has_call_output(body, REPLY_CALL_ID))
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn consult_failure_completes_the_collab_item_with_failed_status() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(ConsultResponder::new(ConsultOutcome::Invalid, false))
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

    let mut created_threads = test.thread_manager.subscribe_thread_created();
    test.submit_turn(ROOT_PROMPT).await?;
    let child_thread_id = tokio::time::timeout(Duration::from_secs(2), created_threads.recv())
        .await
        .context("worker should be created")??;
    let child_thread = test.thread_manager.get_thread(child_thread_id).await?;
    let (saw_in_progress, terminal_item) = tokio::time::timeout(Duration::from_secs(2), async {
        let mut saw_in_progress = false;
        loop {
            let event = child_thread.next_event().await?;
            let item = match event.msg {
                EventMsg::ItemStarted(event) => match event.item {
                    TurnItem::CollabAgentToolCall(item) if item.id == CONSULT_CALL_ID => {
                        saw_in_progress = true;
                        continue;
                    }
                    _ => continue,
                },
                EventMsg::ItemCompleted(event) => match event.item {
                    TurnItem::CollabAgentToolCall(item) if item.id == CONSULT_CALL_ID => item,
                    _ => continue,
                },
                _ => continue,
            };
            return Ok::<_, anyhow::Error>((saw_in_progress, item));
        }
    })
    .await
    .context("consult collab item should reach a terminal state")??;

    assert!(saw_in_progress);
    assert_eq!(terminal_item.tool, CollabAgentTool::AskParent);
    assert_eq!(terminal_item.status, CollabAgentToolCallStatus::Failed);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_consult_cleans_up_without_consuming_real_agent_capacity() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(
            ConsultResponder::new(ConsultOutcome::Advisory, false)
                .with_consult_response_delay(Duration::from_secs(30)),
        )
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
            config.multi_agent_v2.max_concurrent_threads_per_session = 2;
            config.model_provider.supports_websockets = false;
        })
        .build(&server)
        .await?;

    let mut created_threads = test.thread_manager.subscribe_thread_created();
    let submit_turn = test.submit_turn(ROOT_PROMPT);
    tokio::pin!(submit_turn);
    let child_thread_id = tokio::select! {
        child_thread_id = created_threads.recv() => child_thread_id?,
        result = &mut submit_turn => {
            result?;
            return Err(anyhow::anyhow!("root turn completed before consult started"));
        }
    };
    let child_thread = test.thread_manager.get_thread(child_thread_id).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests = server.received_requests().await.unwrap_or_default();
            if requests
                .iter()
                .map(request_body)
                .any(|body| is_consult_responder_request(&body))
            {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .context("consult responder should start within the real-agent execution limit")??;

    child_thread.submit(codex_protocol::protocol::Op::Interrupt).await?;
    wait_for_event(child_thread.as_ref(), |event| {
        matches!(event, EventMsg::TurnAborted(_))
    })
    .await;
    tokio::time::timeout(Duration::from_secs(2), submit_turn.as_mut())
        .await
        .context("root turn should finish after consult cancellation")??;

    assert_eq!(test.thread_manager.list_thread_ids().await.len(), 2);

    Ok(())
}

async fn run_consult(
    outcome: ConsultOutcome,
    request_local_tool: bool,
) -> Result<(
    core_test_support::test_codex::TestCodex,
    wiremock::MockServer,
    Vec<Value>,
    Value,
)> {
    let server = start_mock_server().await;
    Mock::given(method("POST"))
        .and(path_regex(".*/responses$"))
        .respond_with(ConsultResponder::new(outcome, request_local_tool))
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

    let mut created_threads = test.thread_manager.subscribe_thread_created();
    test.submit_turn(ROOT_PROMPT).await?;

    let child_thread_id = tokio::time::timeout(Duration::from_secs(2), created_threads.recv())
        .await
        .context("worker should be created")??;
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let requests = server.received_requests().await.unwrap_or_default();
            if let Some(output) = requests
                .iter()
                .map(request_body)
                .find_map(|body| call_output_text(&body, CONSULT_CALL_ID))
            {
                let result = serde_json::from_str(&output)
                    .with_context(|| format!("consult output was {output:?}"))?;
                return Ok::<_, anyhow::Error>(result);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .context("child should receive consult output")??;
    let child_thread = test.thread_manager.get_thread(child_thread_id).await?;
    wait_for_event(child_thread.as_ref(), |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    let requests = server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|request| request_body(&request))
        .collect();

    Ok((test, server, requests, result))
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

fn final_message_response(response_id: &str, message: &str) -> ResponseTemplate {
    sse_response(sse(vec![
        ev_response_created(response_id),
        ev_assistant_message(&format!("{response_id}-message"), message),
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

fn text_containing(value: &Value, expected: &str) -> Option<String> {
    match value {
        Value::String(text) if text.contains(expected) => Some(text.clone()),
        Value::Array(items) => items
            .iter()
            .find_map(|item| text_containing(item, expected)),
        Value::Object(fields) => fields
            .values()
            .find_map(|item| text_containing(item, expected)),
        Value::String(_) | Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

fn is_consult_responder_request(body: &Value) -> bool {
    body.pointer("/text/format/schema/properties/kind/enum")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values == &[json!("advisory"), json!("requires_authoritative_parent")]
        })
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
