use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadResetContextParams;
use codex_app_server_protocol::ThreadResetContextResponse;
use codex_app_server_protocol::ThreadSource;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::ThreadStartedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput as V2UserInput;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use std::collections::BTreeMap;
use tempfile::TempDir;
use tokio::time::timeout;

#[cfg(any(target_os = "macos", windows))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(any(target_os = "macos", windows)))]
const DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const AUTO_COMPACT_LIMIT: i64 = 1_000;
const COMPACT_PROMPT: &str = "Summarize the conversation.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_reset_context_forks_with_context_without_compaction() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let turn_sse = responses::sse(vec![
        responses::ev_assistant_message("m1", "ORIGINAL_RESPONSE"),
        responses::ev_completed_with_tokens("r1", /*total_tokens*/ 100),
    ]);
    let follow_up_sse = responses::sse(vec![
        responses::ev_assistant_message("m2", "FOLLOW_UP_RESPONSE"),
        responses::ev_completed_with_tokens("r2", /*total_tokens*/ 200),
    ]);
    let response_log = responses::mount_sse_sequence(&server, vec![turn_sse, follow_up_sse]).await;

    let codex_home = TempDir::new()?;
    write_mock_responses_config_toml(
        codex_home.path(),
        &server.uri(),
        &BTreeMap::default(),
        AUTO_COMPACT_LIMIT,
        /*requires_openai_auth*/ None,
        "mock_provider",
        COMPACT_PROMPT,
    )?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let source_thread_id = start_thread(&mut mcp).await?;
    send_turn_and_wait(&mut mcp, &source_thread_id).await?;

    let reset_id = mcp
        .send_thread_reset_context_request(ThreadResetContextParams {
            thread_id: source_thread_id.clone(),
            thread_source: Some(ThreadSource::User),
            ..Default::default()
        })
        .await?;
    let reset_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(reset_id)),
    )
    .await??;
    let ThreadResetContextResponse { thread, .. } =
        to_response::<ThreadResetContextResponse>(reset_resp)?;

    assert_ne!(thread.id, source_thread_id);
    assert_eq!(thread.forked_from_id, Some(source_thread_id.clone()));
    assert_eq!(thread.thread_source, Some(ThreadSource::User));

    let notification = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("thread/started"),
    )
    .await??;
    let started: ThreadStartedNotification =
        serde_json::from_value(notification.params.expect("params must be present"))?;
    assert_eq!(started.thread.id, thread.id);

    send_turn_and_wait(&mut mcp, &thread.id).await?;

    let requests = response_log.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].body_contains_text(COMPACT_PROMPT));
    assert!(requests[1].body_contains_text("ORIGINAL_RESPONSE"));

    Ok(())
}

async fn start_thread(mcp: &mut TestAppServer) -> Result<String> {
    let thread_id = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            thread_source: Some(ThreadSource::User),
            ..Default::default()
        })
        .await?;
    let thread_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(thread_resp)?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("thread/started"),
    )
    .await??;
    Ok(thread.id)
}

async fn send_turn_and_wait(mcp: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let turn_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "build context".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn_resp: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(turn_id)),
    )
    .await??;
    let _turn: TurnStartResponse = to_response::<TurnStartResponse>(turn_resp)?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    Ok(())
}
