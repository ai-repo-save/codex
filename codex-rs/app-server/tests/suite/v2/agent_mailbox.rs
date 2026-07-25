use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::to_response;
use app_test_support::write_models_cache;
use chrono::Utc;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadAgentMailboxGetResponse;
use codex_app_server_protocol::ThreadAgentMailboxUpdatedNotification;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_protocol::ThreadId;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::StateRuntime;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MESSAGE_BODY: &str = "the parent must read this only through agent_mailbox.read";
const CHILD_PROMPT: &str = "send the mailbox result";
const PARENT_SPAWN_PROMPT: &str = "delegate the mailbox result";
const PARENT_READ_PROMPT: &str = "read the durable mailbox";
const SPAWN_CALL_ID: &str = "spawn-mailbox-worker";
const SEND_CALL_ID: &str = "send-mailbox-result";
const READ_CALL_ID: &str = "read-mailbox-result";

#[tokio::test]
async fn thread_agent_mailbox_get_and_resume_hydration_are_count_only() -> Result<()> {
    let responses_server = create_mock_responses_server_repeating_assistant("Done").await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &responses_server.uri())?;
    let state_db = init_state_db(codex_home.path()).await?;
    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, app_server.initialize()).await??;

    let first_thread = start_thread(&mut app_server).await?;
    let second_thread = start_thread(&mut app_server).await?;
    start_turn(&mut app_server, &first_thread.id, "materialize mailbox thread").await?;
    wait_for_turn_completion(&mut app_server, &first_thread.id).await?;
    let first_thread_id = ThreadId::from_string(&first_thread.id)?;
    state_db
        .agent_mailbox()
        .enqueue(AgentMailboxMessageInput {
            id: "agent-mailbox-app-server-test-message".to_string(),
            root_thread_id: first_thread_id,
            sender_thread_id: ThreadId::new(),
            sender_agent_path: "/root/worker".to_string(),
            recipient_thread_id: first_thread_id,
            recipient_agent_path: "/root".to_string(),
            category: AgentMailboxCategory::ActionRequired,
            payload: AgentMailboxPayload::Plaintext {
                content: MESSAGE_BODY.to_string(),
            },
            created_at: Utc::now(),
        })
        .await?;

    let mailbox = get_mailbox(&mut app_server, &first_thread.id).await?;
    assert_eq!(mailbox.total, 1);
    assert_eq!(mailbox.progress, 0);
    assert_eq!(mailbox.result, 0);
    assert_eq!(mailbox.action_required, 1);
    assert_eq!(mailbox.revision, 1);

    let isolated_mailbox = get_mailbox(&mut app_server, &second_thread.id).await?;
    assert_eq!(isolated_mailbox.total, 0);
    assert_eq!(isolated_mailbox.revision, 0);

    let resume_id = app_server
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: first_thread.id.clone(),
            ..Default::default()
        })
        .await?;
    let resume_response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(resume_id)),
    )
    .await??;
    let _: ThreadResumeResponse = to_response(resume_response)?;
    let notification =
        wait_for_mailbox_update(&mut app_server, &first_thread.id, /*revision*/ 1).await?;
    assert_eq!(notification.mailbox.total, 1);
    assert_eq!(notification.mailbox.action_required, 1);

    Ok(())
}

#[tokio::test]
async fn agent_mailbox_body_reaches_parent_only_after_explicit_read() -> Result<()> {
    let server = responses::start_mock_server().await;
    let spawn_args = serde_json::to_string(&json!({
        "message": CHILD_PROMPT,
        "task_name": "mailbox_worker",
    }))?;
    let send_args = serde_json::to_string(&json!({
        "target": "/root",
        "message": MESSAGE_BODY,
        "category": "result",
    }))?;

    let _parent_spawn = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, PARENT_SPAWN_PROMPT),
        responses::sse(vec![
            responses::ev_response_created("resp-parent-spawn"),
            responses::ev_function_call_with_namespace(
                SPAWN_CALL_ID,
                "agents",
                "spawn_agent",
                &spawn_args,
            ),
            responses::ev_completed("resp-parent-spawn"),
        ]),
    )
    .await;
    let _parent_spawn_output = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, SPAWN_CALL_ID),
        responses::sse(vec![
            responses::ev_response_created("resp-parent-spawn-output"),
            responses::ev_assistant_message("msg-parent-spawn-output", "delegated"),
            responses::ev_completed("resp-parent-spawn-output"),
        ]),
    )
    .await;
    let _child_send = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_body_contains(request, CHILD_PROMPT)
                && !request_body_contains(request, SPAWN_CALL_ID)
        },
        responses::sse(vec![
            responses::ev_response_created("resp-child-send"),
            responses::ev_function_call_with_namespace(
                SEND_CALL_ID,
                "agent_mailbox",
                "send",
                &send_args,
            ),
            responses::ev_completed("resp-child-send"),
        ]),
    )
    .await;
    let _child_send_output = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, SEND_CALL_ID),
        responses::sse(vec![
            responses::ev_response_created("resp-child-send-output"),
            responses::ev_assistant_message("msg-child-send-output", "mail queued"),
            responses::ev_completed("resp-child-send-output"),
        ]),
    )
    .await;
    let _parent_read = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| request_body_contains(request, PARENT_READ_PROMPT),
        responses::sse(vec![
            responses::ev_response_created("resp-parent-read"),
            responses::ev_function_call_with_namespace(
                READ_CALL_ID,
                "agent_mailbox",
                "read",
                "{}",
            ),
            responses::ev_completed("resp-parent-read"),
        ]),
    )
    .await;
    let _parent_read_output = responses::mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            request_body_contains(request, READ_CALL_ID)
                && request_body_contains(request, MESSAGE_BODY)
        },
        responses::sse(vec![
            responses::ev_response_created("resp-parent-read-output"),
            responses::ev_assistant_message("msg-parent-read-output", "mail processed"),
            responses::ev_completed("resp-parent-read-output"),
        ]),
    )
    .await;

    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &server.uri())?;
    write_models_cache(codex_home.path())?;
    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, app_server.initialize()).await??;

    let thread = start_thread(&mut app_server).await?;
    start_turn(&mut app_server, &thread.id, PARENT_SPAWN_PROMPT).await?;
    wait_for_turn_completion(&mut app_server, &thread.id).await?;

    let mailbox_update =
        wait_for_mailbox_update(&mut app_server, &thread.id, /*revision*/ 1).await?;
    assert_eq!(mailbox_update.mailbox.total, 1);
    assert_eq!(mailbox_update.mailbox.progress, 0);
    assert_eq!(mailbox_update.mailbox.result, 1);
    assert_eq!(mailbox_update.mailbox.action_required, 0);
    assert_eq!(mailbox_update.mailbox.revision, 1);

    start_turn(&mut app_server, &thread.id, PARENT_READ_PROMPT).await?;
    wait_for_turn_completion(&mut app_server, &thread.id).await?;

    let requests = server
        .received_requests()
        .await
        .expect("mock Responses server should retain requests");
    let read_request = requests
        .iter()
        .find(|request| request_body_contains(request, PARENT_READ_PROMPT))
        .expect("parent read turn should reach Responses");
    assert!(request_body_contains(read_request, "<agent_mailbox>"));
    assert!(request_body_contains(read_request, "1 unread"));
    assert!(!request_body_contains(read_request, MESSAGE_BODY));

    let read_output_request = requests
        .iter()
        .find(|request| request_body_contains(request, READ_CALL_ID))
        .expect("parent read tool output should reach Responses");
    assert!(request_body_contains(read_output_request, MESSAGE_BODY));

    let mailbox = get_mailbox(&mut app_server, &thread.id).await?;
    assert_eq!(mailbox.total, 0);
    assert_eq!(mailbox.progress, 0);
    assert_eq!(mailbox.result, 0);
    assert_eq!(mailbox.action_required, 0);
    assert_eq!(mailbox.revision, 2);

    Ok(())
}

async fn start_thread(app_server: &mut TestAppServer) -> Result<codex_app_server_protocol::Thread> {
    let request_id = app_server
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    Ok(to_response::<ThreadStartResponse>(response)?.thread)
}

async fn start_turn(app_server: &mut TestAppServer, thread_id: &str, prompt: &str) -> Result<()> {
    let request_id = app_server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![V2UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let _: TurnStartResponse = to_response(response)?;
    Ok(())
}

async fn wait_for_turn_completion(app_server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let notification = app_server
                .read_stream_until_notification_message("turn/completed")
                .await?;
            let completed: TurnCompletedNotification =
                serde_json::from_value(notification.params.expect("turn/completed params"))?;
            if completed.thread_id == thread_id {
                return Ok::<(), anyhow::Error>(());
            }
        }
    })
    .await??;
    Ok(())
}

async fn wait_for_mailbox_update(
    app_server: &mut TestAppServer,
    thread_id: &str,
    revision: u64,
) -> Result<ThreadAgentMailboxUpdatedNotification> {
    Ok(timeout(DEFAULT_READ_TIMEOUT, async {
        loop {
            let notification = app_server
                .read_stream_until_notification_message("thread/agentMailbox/updated")
                .await?;
            let params = notification
                .params
                .expect("thread/agentMailbox/updated params");
            assert!(!params.to_string().contains(MESSAGE_BODY));
            let updated: ThreadAgentMailboxUpdatedNotification = serde_json::from_value(params)?;
            if updated.thread_id == thread_id && updated.mailbox.revision >= revision {
                return Ok::<ThreadAgentMailboxUpdatedNotification, anyhow::Error>(updated);
            }
        }
    })
    .await??)
}

async fn get_mailbox(
    app_server: &mut TestAppServer,
    thread_id: &str,
) -> Result<codex_app_server_protocol::AgentMailboxStatus> {
    let request_id = app_server
        .send_raw_request(
            "thread/agentMailbox/get",
            Some(serde_json::json!({ "threadId": thread_id })),
        )
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    Ok(to_response::<ThreadAgentMailboxGetResponse>(response)?.mailbox)
}

async fn init_state_db(codex_home: &Path) -> Result<Arc<StateRuntime>> {
    let state_db = StateRuntime::init(codex_home.to_path_buf(), "mock_provider".into()).await?;
    state_db
        .mark_backfill_complete(/*last_watermark*/ None)
        .await?;
    Ok(state_db)
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"
suppress_unstable_features_warning = true

[features]
sqlite = true
multi_agent_v2 = true
agent_mailbox = true

[model_providers.mock_provider]
name = "Mock provider for test"
base_url = "{server_uri}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#
        ),
    )
}

fn request_body_contains(request: &wiremock::Request, text: &str) -> bool {
    String::from_utf8(request.body.clone())
        .ok()
        .is_some_and(|body| body.contains(text))
}
