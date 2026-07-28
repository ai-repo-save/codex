use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::create_final_assistant_message_sse_response;
use app_test_support::create_mock_responses_server_sequence;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::InitializeCapabilities;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ServerRequest;
use codex_app_server_protocol::SudoOnceRequestApprovalResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput as V2UserInput;
use codex_features::Feature;
use codex_protocol::sudo_once::SudoOnceApprovalDecision;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

const READ_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn sudo_once_approval_uses_the_trusted_experimental_rpc_capability() -> Result<()> {
    let codex_home = TempDir::new()?;
    let responses = vec![
        sudo_once_exec_sse_response("sudo-call")?,
        create_final_assistant_message_sse_response("done")?,
    ];
    let server = create_mock_responses_server_sequence(responses).await;
    write_mock_responses_config_toml(
        codex_home.path(),
        &server.uri(),
        &BTreeMap::from([(Feature::SudoOnce, true)]),
        /*auto_compact_limit*/ 100_000,
        /*requires_openai_auth*/ None,
        "mock_provider",
        "compact",
    )?;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    let initialized = app_server
        .initialize_with_capabilities(
            ClientInfo {
                name: "sudo-once-test-client".to_string(),
                title: None,
                version: "0.1.0".to_string(),
            },
            Some(InitializeCapabilities {
                experimental_api: true,
                sudo_once_credential_prompt: true,
                ..Default::default()
            }),
        )
        .await?;
    assert!(matches!(initialized, JSONRPCMessage::Response(_)));

    let (thread, turn) = start_turn(&mut app_server).await?;
    let request = timeout(READ_TIMEOUT, app_server.read_stream_until_request_message()).await??;
    let ServerRequest::SudoOnceRequestApproval { request_id, params } = request else {
        panic!("expected sudo-once approval request, got: {request:?}");
    };
    assert_eq!(params.thread_id, thread);
    assert_eq!(params.turn_id, turn);
    assert_eq!(params.item_id, "sudo-call");
    assert!(params.command.contains("printf"));
    assert_eq!(params.reason.as_deref(), Some("needs elevated access"));

    app_server
        .send_response(
            request_id,
            serde_json::to_value(SudoOnceRequestApprovalResponse {
                decision: SudoOnceApprovalDecision::Abort,
            })?,
        )
        .await?;

    let completed = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_notification_message("turn/completed"),
    )
    .await??;
    let completed: TurnCompletedNotification =
        serde_json::from_value(completed.params.expect("turn/completed must have params"))?;
    assert_eq!(completed.thread_id, thread);
    assert_eq!(completed.turn.id, turn);
    assert_eq!(completed.turn.status, TurnStatus::Completed);

    Ok(())
}

async fn start_turn(app_server: &mut TestAppServer) -> Result<(String, String)> {
    let thread_request_id = app_server
        .send_thread_start_request_with_auto_env(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let thread_response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(thread_request_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(thread_response)?;

    let turn_request_id = app_server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id.clone(),
            client_user_message_id: None,
            input: vec![V2UserInput::Text {
                text: "run a privileged command".to_string(),
                text_elements: Vec::new(),
            }],
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let turn_response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(turn_request_id)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response(turn_response)?;

    Ok((thread.id, turn.id))
}

fn sudo_once_exec_sse_response(call_id: &str) -> Result<String> {
    let arguments = serde_json::to_string(&json!({
        "cmd": "printf privileged",
        "privilege": "sudo_once",
        "justification": "needs elevated access",
        "yield_time_ms": 1_000,
    }))?;
    Ok(responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_function_call(call_id, "exec_command", &arguments),
        responses::ev_completed("resp-1"),
    ]))
}
