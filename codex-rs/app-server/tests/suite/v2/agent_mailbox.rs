use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use app_test_support::to_response;
use chrono::Utc;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadAgentMailboxGetResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_protocol::ThreadId;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::StateRuntime;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MESSAGE_BODY: &str = "the parent must read this only through agent_mailbox.read";

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
    let notification: JSONRPCNotification = timeout(
        DEFAULT_READ_TIMEOUT,
        app_server.read_stream_until_notification_message("thread/agentMailbox/updated"),
    )
    .await??;
    let notification_json = serde_json::to_string(&notification)?;
    assert!(notification_json.contains("\"total\":1"));
    assert!(notification_json.contains("\"actionRequired\":1"));
    assert!(!notification_json.contains(MESSAGE_BODY));

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
