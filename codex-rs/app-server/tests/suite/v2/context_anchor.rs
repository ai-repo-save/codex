use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::ItemStartedNotification;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn save_context_anchor_emits_started_and_completed_items() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_function_call(
                "save-anchor-call",
                "save_context_anchor",
                &serde_json::json!({ "label": "before branch" }).to_string(),
            ),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;

    let codex_home = TempDir::new()?;
    write_mock_responses_config_toml(
        codex_home.path(),
        &server.uri(),
        &BTreeMap::default(),
        /*model_context_window*/ 200_000,
        /*requires_openai_auth*/ None,
        "mock_provider",
        "Summarize the conversation.",
    )?;

    let mut mcp = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build()
        .await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let thread_id = start_thread(&mut mcp).await?;
    let turn_id = start_turn(&mut mcp, &thread_id).await?;

    let started = wait_for_context_anchor_saved_started(&mut mcp).await?;
    let completed = wait_for_context_anchor_saved_completed(&mut mcp).await?;
    wait_for_turn_completed(&mut mcp, &turn_id).await?;

    let ThreadItem::ContextAnchorSaved {
        id: started_id,
        anchor_id: started_anchor_id,
        label: started_label,
        ..
    } = started.item
    else {
        panic!("expected contextAnchorSaved started item");
    };
    let ThreadItem::ContextAnchorSaved {
        id: completed_id,
        anchor_id: completed_anchor_id,
        label: completed_label,
        ..
    } = completed.item
    else {
        panic!("expected contextAnchorSaved completed item");
    };

    assert_eq!(started.thread_id, thread_id);
    assert_eq!(completed.thread_id, thread_id);
    assert_eq!(started_id, completed_id);
    assert_eq!(started_anchor_id, completed_anchor_id);
    assert_eq!(started_label, Some("before branch".to_string()));
    assert_eq!(completed_label, Some("before branch".to_string()));

    Ok(())
}

async fn start_thread(mcp: &mut TestAppServer) -> Result<String> {
    let request_id = mcp
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(response)?;
    Ok(thread.id)
}

async fn start_turn(mcp: &mut TestAppServer, thread_id: &str) -> Result<String> {
    let request_id = mcp
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            client_user_message_id: None,
            input: vec![UserInput::Text {
                text: "save an anchor".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response(response)?;
    Ok(turn.id)
}

async fn wait_for_context_anchor_saved_started(
    mcp: &mut TestAppServer,
) -> Result<ItemStartedNotification> {
    loop {
        let notification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_notification_message("item/started"),
        )
        .await??;
        let started: ItemStartedNotification =
            serde_json::from_value(notification.params.expect("item/started params"))?;
        if matches!(started.item, ThreadItem::ContextAnchorSaved { .. }) {
            return Ok(started);
        }
    }
}

async fn wait_for_context_anchor_saved_completed(
    mcp: &mut TestAppServer,
) -> Result<ItemCompletedNotification> {
    loop {
        let notification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_notification_message("item/completed"),
        )
        .await??;
        let completed: ItemCompletedNotification =
            serde_json::from_value(notification.params.expect("item/completed params"))?;
        if matches!(completed.item, ThreadItem::ContextAnchorSaved { .. }) {
            return Ok(completed);
        }
    }
}

async fn wait_for_turn_completed(mcp: &mut TestAppServer, turn_id: &str) -> Result<()> {
    loop {
        let notification: JSONRPCNotification = timeout(
            DEFAULT_READ_TIMEOUT,
            mcp.read_stream_until_notification_message("turn/completed"),
        )
        .await??;
        let completed: TurnCompletedNotification =
            serde_json::from_value(notification.params.expect("turn/completed params"))?;
        if completed.turn.id == turn_id {
            return Ok(());
        }
    }
}
