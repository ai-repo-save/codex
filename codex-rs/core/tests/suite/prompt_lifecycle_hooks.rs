use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use codex_core::compact::SUMMARIZATION_PROMPT;
use codex_features::Feature;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookRunStatus;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::user_input::UserInput;
use core_test_support::hooks::trust_discovered_hooks;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::time::Instant;
use tokio::time::sleep;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

const MAIN_MODEL: &str = "prompt-lifecycle-main-model";
const EVALUATOR_MODEL: &str = "prompt-lifecycle-evaluator-model";
const EVALUATOR_PROMPT: &str = "Evaluate this lifecycle hook payload: $$ARGUMENTS";
const FIRST_PROMPT: &str = "first lifecycle prompt";
const NEXT_PROMPT: &str = "next lifecycle prompt";
const FIRST_REPLY: &str = "first lifecycle reply";
const NEXT_REPLY: &str = "next lifecycle reply";
const SPAWN_PROMPT: &str = "spawn the lifecycle child";
const CHILD_PROMPT: &str = "run the lifecycle child";
const SPAWN_CALL_ID: &str = "prompt-lifecycle-spawn";
const COLLAB_SPAWN_HEADER_VALUE: &str = "collab_spawn";

fn write_prompt_hook(
    home: &Path,
    event_name: &str,
    matcher: Option<&str>,
    fail_closed: bool,
) -> Result<()> {
    let mut registration = json!({
        "hooks": [{
            "type": "prompt",
            "prompt": EVALUATOR_PROMPT,
            "model": EVALUATOR_MODEL,
            "failClosed": fail_closed,
        }]
    });
    if let Some(matcher) = matcher {
        registration["matcher"] = json!(matcher);
    }
    let hooks = json!({
        "hooks": {
            (event_name): [registration],
        }
    });
    fs::write(home.join("hooks.json"), hooks.to_string()).context("write hooks.json")?;
    Ok(())
}

fn model_sse(id: &str, output: &str) -> String {
    sse(vec![
        ev_response_created(id),
        ev_assistant_message(&format!("{id}-message"), output),
        ev_completed(id),
    ])
}

async fn submit_turn_and_collect(test: &TestCodex, prompt: &str) -> Result<Vec<EventMsg>> {
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        })
        .await?;

    let mut turn_id = None;
    let mut events = Vec::new();
    loop {
        let event = test.codex.next_event().await?.msg;
        if let EventMsg::TurnStarted(started) = &event {
            turn_id = Some(started.turn_id.clone());
        }
        let terminal = match &event {
            EventMsg::TurnComplete(completed) => turn_id.as_ref() == Some(&completed.turn_id),
            EventMsg::TurnAborted(aborted) => aborted.turn_id.as_ref() == turn_id.as_ref(),
            _ => false,
        };
        events.push(event);
        if terminal {
            return Ok(events);
        }
    }
}

fn failed_prompt_hook(events: &[EventMsg], event_name: HookEventName) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            EventMsg::HookCompleted(completed)
                if completed.run.event_name == event_name
                    && completed.run.handler_type == HookHandlerType::Prompt
                    && completed.run.status == HookRunStatus::Failed
        )
    })
}

fn completed_without_message(events: &[EventMsg]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            EventMsg::TurnComplete(completed) if completed.last_agent_message.is_none()
        )
    })
}

fn request_uses_model(request: &ResponsesRequest, model: &str) -> bool {
    request.body_json()["model"] == json!(model)
}

fn decoded_body(request: &wiremock::Request) -> Option<Vec<u8>> {
    let is_zstd = request
        .headers
        .get("content-encoding")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|entry| entry.trim().eq_ignore_ascii_case("zstd"))
        });
    if is_zstd {
        zstd::stream::decode_all(std::io::Cursor::new(&request.body)).ok()
    } else {
        Some(request.body.clone())
    }
}

fn request_body(request: &wiremock::Request) -> Option<Value> {
    decoded_body(request).and_then(|body| serde_json::from_slice(&body).ok())
}

fn request_header<'a>(request: &'a wiremock::Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
}

fn request_contains(request: &wiremock::Request, text: &str) -> bool {
    decoded_body(request)
        .and_then(|body| String::from_utf8(body).ok())
        .is_some_and(|body| body.contains(text))
}

async fn wait_for_child(test: &TestCodex) -> Result<std::sync::Arc<codex_core::CodexThread>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(thread_id) = test
            .thread_manager
            .list_thread_ids()
            .await
            .into_iter()
            .find(|thread_id| thread_id != &test.session_configured.thread_id)
        {
            return Ok(test.thread_manager.get_thread(thread_id).await?);
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for lifecycle child");
        }
        sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_compact_invalid_prompt_output_fails_open_and_compacts() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            model_sse("pre-compact-open-seed", FIRST_REPLY),
            model_sse("pre-compact-open-evaluator", "not json"),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(MAIN_MODEL)
        .with_pre_build_hook(|home| {
            write_prompt_hook(home, "PreCompact", Some("manual"), /*fail_closed*/ false)
                .expect("write PreCompact prompt hook");
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
            let _ = config.features.disable(Feature::RemoteCompactionV2);
        })
        .build(&server)
        .await?;

    test.submit_turn(FIRST_PROMPT).await?;
    test.codex.submit(Op::Compact).await?;
    let events = wait_for_compact_terminal(&test).await?;

    assert!(failed_prompt_hook(&events, HookEventName::PreCompact));
    assert!(matches!(events.last(), Some(EventMsg::TurnComplete(_))));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::TurnAborted(_)))
    );
    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(request_uses_model(&requests[0], MAIN_MODEL));
    assert!(request_uses_model(&requests[1], EVALUATOR_MODEL));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pre_compact_fail_closed_aborts_only_compaction() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            model_sse("pre-compact-closed-seed", FIRST_REPLY),
            model_sse("pre-compact-closed-evaluator", "not json"),
            model_sse("pre-compact-closed-next", NEXT_REPLY),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(MAIN_MODEL)
        .with_pre_build_hook(|home| {
            write_prompt_hook(home, "PreCompact", Some("manual"), /*fail_closed*/ true)
                .expect("write PreCompact prompt hook");
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config.compact_prompt = Some(SUMMARIZATION_PROMPT.to_string());
            let _ = config.features.disable(Feature::RemoteCompactionV2);
        })
        .build(&server)
        .await?;

    test.submit_turn(FIRST_PROMPT).await?;
    test.codex.submit(Op::Compact).await?;
    let events = wait_for_compact_terminal(&test).await?;

    assert!(failed_prompt_hook(&events, HookEventName::PreCompact));
    assert!(events.iter().any(|event| {
        matches!(
            event,
            EventMsg::TurnAborted(aborted) if aborted.reason == TurnAbortReason::Interrupted
        )
    }));
    test.submit_turn(NEXT_PROMPT).await?;
    let requests = responses.requests();
    assert_eq!(requests.len(), 3);
    assert!(request_uses_model(&requests[0], MAIN_MODEL));
    assert!(request_uses_model(&requests[1], EVALUATOR_MODEL));
    assert!(request_uses_model(&requests[2], MAIN_MODEL));
    Ok(())
}

async fn wait_for_compact_terminal(test: &TestCodex) -> Result<Vec<EventMsg>> {
    let mut turn_id = None;
    let mut events = Vec::new();
    loop {
        let event = test.codex.next_event().await?.msg;
        if let EventMsg::TurnStarted(started) = &event {
            turn_id = Some(started.turn_id.clone());
        }
        let terminal = match &event {
            EventMsg::TurnComplete(completed) => turn_id.as_ref() == Some(&completed.turn_id),
            EventMsg::TurnAborted(aborted) => turn_id
                .as_ref()
                .is_some_and(|turn_id| aborted.turn_id.as_ref() == Some(turn_id)),
            _ => false,
        };
        events.push(event);
        if terminal {
            return Ok(events);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_start_fail_closed_skips_first_sample_and_next_turn_continues() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            model_sse("session-start-evaluator", "not json"),
            model_sse("session-start-next", NEXT_REPLY),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(MAIN_MODEL)
        .with_pre_build_hook(|home| {
            write_prompt_hook(home, "SessionStart", Some("startup"), /*fail_closed*/ true)
                .expect("write SessionStart prompt hook");
        })
        .with_config(trust_discovered_hooks)
        .build(&server)
        .await?;

    let events = submit_turn_and_collect(&test, FIRST_PROMPT).await?;
    assert!(failed_prompt_hook(&events, HookEventName::SessionStart));
    assert!(completed_without_message(&events));
    test.submit_turn(NEXT_PROMPT).await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 2);
    assert!(request_uses_model(&requests[0], EVALUATOR_MODEL));
    assert!(request_uses_model(&requests[1], MAIN_MODEL));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_submit_fail_closed_rejects_only_the_failed_input() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let responses = mount_sse_sequence(
        &server,
        vec![
            model_sse("user-prompt-evaluator-failed", "not json"),
            model_sse("user-prompt-evaluator-passed", "{}"),
            model_sse("user-prompt-next", NEXT_REPLY),
        ],
    )
    .await;
    let test = test_codex()
        .with_model(MAIN_MODEL)
        .with_pre_build_hook(|home| {
            write_prompt_hook(
                home,
                "UserPromptSubmit",
                /*matcher*/ None,
                /*fail_closed*/ true,
            )
                .expect("write UserPromptSubmit prompt hook");
        })
        .with_config(trust_discovered_hooks)
        .build(&server)
        .await?;

    let events = submit_turn_and_collect(&test, FIRST_PROMPT).await?;
    assert!(failed_prompt_hook(&events, HookEventName::UserPromptSubmit));
    assert!(completed_without_message(&events));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventMsg::UserMessage(_)))
    );
    test.submit_turn(NEXT_PROMPT).await?;

    let requests = responses.requests();
    assert_eq!(requests.len(), 3);
    assert!(request_uses_model(&requests[0], EVALUATOR_MODEL));
    assert!(request_uses_model(&requests[1], EVALUATOR_MODEL));
    assert!(request_uses_model(&requests[2], MAIN_MODEL));
    assert!(!requests[2].body_contains_text(FIRST_PROMPT));
    assert!(requests[2].body_contains_text(NEXT_PROMPT));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subagent_start_fail_closed_errors_child_without_sampling_or_prewarm() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    Mock::given(method("GET"))
        .and(path_regex(".*/responses$"))
        .respond_with(ResponseTemplate::new(426))
        .mount(&server)
        .await;
    let spawn_args = serde_json::to_string(&json!({
        "message": CHILD_PROMPT,
        "task_name": "worker",
    }))?;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_contains(request, SPAWN_PROMPT),
        sse(vec![
            ev_response_created("subagent-parent-spawn"),
            ev_function_call_with_namespace(SPAWN_CALL_ID, "agents", "spawn_agent", &spawn_args),
            ev_completed("subagent-parent-spawn"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_body(request).is_some_and(|body| body["model"] == json!(EVALUATOR_MODEL))
        },
        model_sse("subagent-start-evaluator", "not json"),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_body(request).is_some_and(|body| {
                body["model"] == json!(MAIN_MODEL)
                    && request_header(request, "x-openai-subagent")
                        == Some(COLLAB_SPAWN_HEADER_VALUE)
            })
        },
        model_sse("unexpected-child-main", "unexpected child sample"),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_contains(request, SPAWN_CALL_ID),
        model_sse("subagent-parent-followup", FIRST_REPLY),
    )
    .await;

    let test = test_codex()
        .with_model(MAIN_MODEL)
        .with_pre_build_hook(|home| {
            write_prompt_hook(home, "SubagentStart", Some("default"), /*fail_closed*/ true)
                .expect("write SubagentStart prompt hook");
        })
        .with_config(|config| {
            trust_discovered_hooks(config);
            config
                .features
                .enable(Feature::Collab)
                .expect("enable collaboration");
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("enable multi-agent v2");
            config.model_provider.supports_websockets = true;
        })
        .build(&server)
        .await?;

    test.submit_turn(SPAWN_PROMPT).await?;
    let child = wait_for_child(&test).await?;
    let child_error = wait_for_event_match(&child, |event| match event {
        EventMsg::Error(error) => Some(error.clone()),
        _ => None,
    })
    .await;
    assert_eq!(
        child_error.codex_error_info,
        Some(CodexErrorInfo::Other),
        "unexpected child error: {child_error:?}"
    );
    assert!(matches!(
        child.agent_status().await,
        AgentStatus::Errored(_)
    ));
    let requests = server
        .received_requests()
        .await
        .context("read recorded lifecycle requests")?;
    let evaluator_request_count = requests
        .iter()
        .filter(|request| {
            request.method == "POST"
                && request.url.path().ends_with("/responses")
                && request_body(request).is_some_and(|body| {
                    body["model"] == json!(EVALUATOR_MODEL)
                        && body
                            .get("tools")
                            .is_none_or(|tools| tools.as_array().is_some_and(Vec::is_empty))
                })
        })
        .count();
    let child_main_sampling_request_count = requests
        .iter()
        .filter(|request| {
            request.method == "POST"
                && request.url.path().ends_with("/responses")
                && request_header(request, "x-openai-subagent")
                    == Some(COLLAB_SPAWN_HEADER_VALUE)
                && request_body(request)
                    .is_some_and(|body| body["model"] == json!(MAIN_MODEL))
        })
        .count();
    let child_websocket_prewarm_request_count = requests
        .iter()
        .filter(|request| {
            request.method == "GET"
                && request.url.path().ends_with("/responses")
                && request_header(request, "x-openai-subagent")
                    == Some(COLLAB_SPAWN_HEADER_VALUE)
        })
        .count();
    assert_eq!(
        evaluator_request_count, 1,
        "unexpected child error: {child_error:?}"
    );
    assert_eq!(
        child_main_sampling_request_count, 0,
        "rejected child sent main-model sampling traffic"
    );
    assert_eq!(
        child_websocket_prewarm_request_count, 0,
        "rejected child sent websocket startup prewarm traffic"
    );
    Ok(())
}
