use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_mock_responses_config_toml;
use codex_app_server_protocol::ItemCompletedNotification;
use codex_app_server_protocol::JSONRPCNotification;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

#[cfg(any(target_os = "macos", windows))]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
#[cfg(not(any(target_os = "macos", windows)))]
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MEMORY_NAMESPACE: &str = "memories";
const WRITE_NOTE_TOOL_NAME: &str = "write_note";
const DELETE_TOOL_NAME: &str = "delete";
const SAVE_CONTEXT_ANCHOR_TOOL_NAME: &str = "save_context_anchor";
const REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME: &str = "rewind_context_to_anchor";
const FIRST_MEMORY_NOTE: &str = "Remember the first post-anchor session fact.";
const SECOND_MEMORY_NOTE: &str = "Remember the second post-anchor session fact.";
const DELETED_MEMORY_NOTE: &str = "This deleted post-anchor session fact must stay absent.";
const FIRST_REWIND_NOTE: &str = "Continue after the first memory rewind.";
const SECOND_REWIND_NOTE: &str = "Continue after the second memory rewind.";
const DELETED_REWIND_NOTE: &str = "Continue without the deleted memory.";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rewind_injects_each_new_session_memory_once_and_persists_it_on_resume() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    write_scoped_memory_config(codex_home.path(), &server.uri())?;
    let mut app_server = build_app_server(codex_home.path()).await?;
    let thread_id = start_thread(&mut app_server).await?;
    let anchor_id = save_anchor(
        &server,
        &mut app_server,
        &thread_id,
        "save-memory-anchor-call",
    )
    .await?;

    let first_rewind_call_id = "rewind-first-memory-call";
    let first_rewind_mock = responses::mount_sse_sequence(
        &server,
        vec![
            write_session_note_response(
                "write-first-response",
                "write-first-memory-call",
                "first post-anchor memory",
                FIRST_MEMORY_NOTE,
            ),
            rewind_response(
                "rewind-first-response",
                first_rewind_call_id,
                &anchor_id,
                FIRST_REWIND_NOTE,
            ),
            assistant_response("first-rewind-complete", "first-rewind-message"),
        ],
    )
    .await;
    run_turn(&mut app_server, &thread_id, "write and rewind once").await?;
    let first_rewind_requests = first_rewind_mock.requests();
    assert_eq!(first_rewind_requests.len(), 3);
    assert_user_text_occurrences(
        &first_rewind_requests[2],
        &[(FIRST_MEMORY_NOTE, 1), (FIRST_REWIND_NOTE, 1)],
    );
    let replacement_anchor_id =
        replacement_anchor_id(&first_rewind_requests[2], first_rewind_call_id)?;

    let second_rewind_mock = responses::mount_sse_sequence(
        &server,
        vec![
            write_session_note_response(
                "write-second-response",
                "write-second-memory-call",
                "second post-anchor memory",
                SECOND_MEMORY_NOTE,
            ),
            rewind_response(
                "rewind-second-response",
                "rewind-second-memory-call",
                &replacement_anchor_id,
                SECOND_REWIND_NOTE,
            ),
            assistant_response("second-rewind-complete", "second-rewind-message"),
        ],
    )
    .await;
    run_turn(&mut app_server, &thread_id, "write and rewind twice").await?;
    let second_rewind_requests = second_rewind_mock.requests();
    assert_eq!(second_rewind_requests.len(), 3);
    assert_user_text_occurrences(
        &second_rewind_requests[2],
        &[
            (FIRST_MEMORY_NOTE, 1),
            (SECOND_MEMORY_NOTE, 1),
            (SECOND_REWIND_NOTE, 1),
        ],
    );

    drop(app_server);
    let resume_mock = responses::mount_sse_once(
        &server,
        assistant_response("resume-response", "resume-message"),
    )
    .await;
    let mut resumed_app_server = build_app_server(codex_home.path()).await?;
    resume_thread(&mut resumed_app_server, &thread_id).await?;
    run_turn(
        &mut resumed_app_server,
        &thread_id,
        "continue after resume",
    )
    .await?;
    assert_user_text_occurrences(
        &resume_mock.single_request(),
        &[(FIRST_MEMORY_NOTE, 1), (SECOND_MEMORY_NOTE, 1)],
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rewind_skips_session_memory_deleted_after_its_write() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = responses::start_mock_server().await;
    let codex_home = TempDir::new()?;
    write_scoped_memory_config(codex_home.path(), &server.uri())?;
    let mut app_server = build_app_server(codex_home.path()).await?;
    let thread_id = start_thread(&mut app_server).await?;
    let anchor_id = save_anchor(
        &server,
        &mut app_server,
        &thread_id,
        "save-delete-anchor-call",
    )
    .await?;

    let write_call_id = "write-deleted-memory-call";
    let write_mock = responses::mount_sse_sequence(
        &server,
        vec![
            write_session_note_response(
                "write-deleted-response",
                write_call_id,
                "deleted post-anchor memory",
                DELETED_MEMORY_NOTE,
            ),
            assistant_response("write-deleted-complete", "write-deleted-message"),
        ],
    )
    .await;
    run_turn(
        &mut app_server,
        &thread_id,
        "write memory before deleting it",
    )
    .await?;
    let write_requests = write_mock.requests();
    let write_output = write_requests[1]
        .function_call_output_text(write_call_id)
        .expect("write_note output should be text JSON");
    let write_output: serde_json::Value = serde_json::from_str(&write_output)?;
    let path = write_output["path"]
        .as_str()
        .expect("write_note output should include a path");

    let delete_mock = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("delete-response"),
                responses::ev_function_call_with_namespace(
                    "delete-memory-call",
                    MEMORY_NAMESPACE,
                    DELETE_TOOL_NAME,
                    &serde_json::json!({"scope": "session", "path": path}).to_string(),
                ),
                responses::ev_completed("delete-response"),
            ]),
            assistant_response("delete-complete", "delete-message"),
        ],
    )
    .await;
    run_turn(&mut app_server, &thread_id, "delete the written memory").await?;
    assert_eq!(delete_mock.requests().len(), 2);

    let rewind_mock = responses::mount_sse_sequence(
        &server,
        vec![
            rewind_response(
                "rewind-deleted-response",
                "rewind-deleted-memory-call",
                &anchor_id,
                DELETED_REWIND_NOTE,
            ),
            assistant_response("rewind-deleted-complete", "rewind-deleted-message"),
        ],
    )
    .await;
    run_turn(
        &mut app_server,
        &thread_id,
        "rewind after deleting memory",
    )
    .await?;
    let rewind_requests = rewind_mock.requests();
    assert_eq!(rewind_requests.len(), 2);
    assert_user_text_occurrences(
        &rewind_requests[1],
        &[(DELETED_MEMORY_NOTE, 0), (DELETED_REWIND_NOTE, 1)],
    );

    Ok(())
}

async fn build_app_server(codex_home: &Path) -> Result<TestAppServer> {
    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home)
        .without_auto_env()
        .build()
        .await?;
    timeout(READ_TIMEOUT, app_server.initialize()).await??;
    Ok(app_server)
}

async fn start_thread(app_server: &mut TestAppServer) -> Result<String> {
    let request_id = app_server
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(response)?;
    Ok(thread.id)
}

async fn save_anchor(
    server: &wiremock::MockServer,
    app_server: &mut TestAppServer,
    thread_id: &str,
    call_id: &str,
) -> Result<String> {
    responses::mount_sse_sequence(
        server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("save-response"),
                responses::ev_function_call(
                    call_id,
                    SAVE_CONTEXT_ANCHOR_TOOL_NAME,
                    &serde_json::json!({ "label": "before session memories" }).to_string(),
                ),
                responses::ev_completed("save-response"),
            ]),
            assistant_response("save-complete-response", "save-complete-message"),
        ],
    )
    .await;
    let turn_id = start_turn(app_server, thread_id, "save the memory baseline").await?;
    let completed = wait_for_context_anchor_saved_completed(app_server).await?;
    wait_for_turn_completed(app_server, &turn_id).await?;
    let ThreadItem::ContextAnchorSaved { anchor_id, .. } = completed.item else {
        panic!("expected contextAnchorSaved completed item");
    };
    Ok(anchor_id)
}

async fn run_turn(app_server: &mut TestAppServer, thread_id: &str, prompt: &str) -> Result<()> {
    let turn_id = start_turn(app_server, thread_id, prompt).await?;
    wait_for_turn_completed(app_server, &turn_id).await
}

async fn start_turn(
    app_server: &mut TestAppServer,
    thread_id: &str,
    prompt: &str,
) -> Result<String> {
    let request_id = app_server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let TurnStartResponse { turn } = to_response(response)?;
    Ok(turn.id)
}

async fn resume_thread(app_server: &mut TestAppServer, thread_id: &str) -> Result<()> {
    let request_id = app_server
        .send_thread_resume_request(ThreadResumeParams {
            thread_id: thread_id.to_string(),
            ..Default::default()
        })
        .await?;
    let response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let ThreadResumeResponse { thread, .. } = to_response(response)?;
    assert_eq!(thread.id, thread_id);
    Ok(())
}

fn assistant_response(response_id: &str, message_id: &str) -> String {
    responses::sse(vec![
        responses::ev_assistant_message(message_id, "done"),
        responses::ev_completed(response_id),
    ])
}

fn write_session_note_response(
    response_id: &str,
    call_id: &str,
    title: &str,
    note: &str,
) -> String {
    responses::sse(vec![
        responses::ev_response_created(response_id),
        responses::ev_function_call_with_namespace(
            call_id,
            MEMORY_NAMESPACE,
            WRITE_NOTE_TOOL_NAME,
            &serde_json::json!({"scope": "session", "title": title, "note": note}).to_string(),
        ),
        responses::ev_completed(response_id),
    ])
}

fn rewind_response(
    response_id: &str,
    call_id: &str,
    anchor_id: &str,
    note: &str,
) -> String {
    responses::sse(vec![
        responses::ev_response_created(response_id),
        responses::ev_function_call(
            call_id,
            REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
            &serde_json::json!({"anchor_id": anchor_id, "note": note}).to_string(),
        ),
        responses::ev_completed(response_id),
    ])
}

fn replacement_anchor_id(
    request: &responses::ResponsesRequest,
    call_id: &str,
) -> Result<String> {
    let output = request
        .function_call_output_text(call_id)
        .expect("rewind output should be text JSON");
    let output: serde_json::Value = serde_json::from_str(&output)?;
    Ok(output["replacement_anchor_id"]
        .as_str()
        .expect("successful rewind should include a replacement anchor id")
        .to_string())
}

fn assert_user_text_occurrences(
    request: &responses::ResponsesRequest,
    expected_occurrences: &[(&str, usize)],
) {
    let user_text = request.message_input_texts("user").join("\n");
    for (text, expected_count) in expected_occurrences {
        assert_eq!(
            user_text.matches(text).count(),
            *expected_count,
            "unexpected occurrence count for {text:?} in user context"
        );
    }
}

fn write_scoped_memory_config(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    let mut features = BTreeMap::new();
    features.insert(Feature::MemoryTool, true);
    write_mock_responses_config_toml(
        codex_home,
        server_uri,
        &features,
        /*auto_compact_limit*/ 200_000,
        /*requires_openai_auth*/ None,
        "mock_provider",
        "Summarize the conversation.",
    )?;
    let config_path = codex_home.join("config.toml");
    let config = std::fs::read_to_string(&config_path)?;
    std::fs::write(
        config_path,
        format!(
            r#"{config}
[memories]
generate_memories = false
use_memories = false
use_scoped_memories = true
dedicated_tools = true
"#
        ),
    )
}

async fn wait_for_context_anchor_saved_completed(
    app_server: &mut TestAppServer,
) -> Result<ItemCompletedNotification> {
    loop {
        let notification = timeout(
            READ_TIMEOUT,
            app_server.read_stream_until_notification_message("item/completed"),
        )
        .await??;
        let completed: ItemCompletedNotification =
            serde_json::from_value(notification.params.expect("item/completed params"))?;
        if matches!(completed.item, ThreadItem::ContextAnchorSaved { .. }) {
            return Ok(completed);
        }
    }
}

async fn wait_for_turn_completed(app_server: &mut TestAppServer, turn_id: &str) -> Result<()> {
    loop {
        let notification: JSONRPCNotification = timeout(
            READ_TIMEOUT,
            app_server.read_stream_until_notification_message("turn/completed"),
        )
        .await??;
        let completed: TurnCompletedNotification =
            serde_json::from_value(notification.params.expect("turn/completed params"))?;
        if completed.turn.id == turn_id {
            return Ok(());
        }
    }
}
