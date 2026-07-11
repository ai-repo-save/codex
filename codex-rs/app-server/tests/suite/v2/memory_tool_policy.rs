use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use app_test_support::write_models_cache_with_models;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use codex_models_manager::model_info::model_info_from_slug;
use codex_protocol::openai_models::ToolMode;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use tokio::time::timeout;

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MODEL: &str = "gpt-5.6-terra";
const WRITE_NOTE_TOOL: &str = "memories__write_note";
const DELETE_TOOL: &str = "memories__delete";
const SESSION_PROACTIVE_POLICY: &str = "no explicit user request is required";
const PROJECT_POLICY_AUTHORITY: &str = "project AGENTS.md instructions authorize";
const GLOBAL_EXPLICIT_POLICY: &str = "only when the user explicitly asks Codex";
const OLD_SCOPED_EXPLICIT_POLICY: &str = "after the user explicitly asks Codex to remember, forget, or update something for this session or project";
const GLOBAL_UPDATE_HEADING: &str = "Updating global memories:";
const GLOBAL_DELETE_SCOPE: &str = "with `scope: \"global\"`";
const OLD_UNSCOPED_UPDATE_GATE: &str = "You can update the memories **only**";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_memory_policy_reaches_responses_lite_as_developer_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let responses_server = responses::start_mock_server().await;
    let response_body = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        responses::ev_completed("resp-1"),
    ]);
    let response_mock = responses::mount_sse_sequence(
        &responses_server,
        vec![response_body.clone(), response_body],
    )
    .await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &responses_server.uri())?;
    create_global_memory_summary(codex_home.path())?;
    let mut model_info = model_info_from_slug(MODEL);
    model_info.use_responses_lite = true;
    model_info.tool_mode = Some(ToolMode::CodeMode);
    write_models_cache_with_models(codex_home.path(), vec![model_info])?;

    let mut app_server = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .build()
        .await?;
    timeout(READ_TIMEOUT, app_server.initialize()).await??;

    let thread_request_id = app_server
        .send_thread_start_request_with_auto_env(ThreadStartParams::default())
        .await?;
    let thread_response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(thread_request_id)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response(thread_response)?;

    let turn_request_id = app_server
        .send_turn_start_request(TurnStartParams {
            thread_id: thread.id,
            input: vec![UserInput::Text {
                text: "Use the available project context.".to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        })
        .await?;
    let turn_response: JSONRPCResponse = timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_response_message(RequestId::Integer(turn_request_id)),
    )
    .await??;
    let _: TurnStartResponse = to_response(turn_response)?;
    timeout(
        READ_TIMEOUT,
        app_server.read_stream_until_notification_message("turn/completed"),
    )
    .await??;

    let request_body = response_mock
        .requests()
        .iter()
        .find_map(|request| {
            let body = request.body_json();
            let additional_tools = body["input"].as_array()?.first()?;
            let developer_tools = serde_json::to_string(&additional_tools["tools"]).ok()?;
            (additional_tools["type"] == "additional_tools"
                && additional_tools["role"] == "developer"
                && developer_tools.contains(WRITE_NOTE_TOOL)
                && developer_tools.contains(SESSION_PROACTIVE_POLICY))
            .then_some(body)
        })
        .context("Responses request log should include scoped memory developer tools")?;
    assert!(request_body.get("tools").is_none());
    let input = request_body["input"]
        .as_array()
        .context("Responses input should be an array")?;
    let additional_tools = input
        .first()
        .context("Responses input should include additional tools")?;
    assert_eq!(additional_tools["type"], "additional_tools");
    assert_eq!(additional_tools["role"], "developer");

    let developer_tools = serde_json::to_string(&additional_tools["tools"])?;
    assert!(developer_tools.contains(WRITE_NOTE_TOOL));
    assert!(developer_tools.contains(DELETE_TOOL));
    assert!(developer_tools.contains(SESSION_PROACTIVE_POLICY));
    assert!(developer_tools.contains(PROJECT_POLICY_AUTHORITY));
    assert!(developer_tools.contains(GLOBAL_EXPLICIT_POLICY));
    assert!(!developer_tools.contains(OLD_SCOPED_EXPLICIT_POLICY));
    let developer_messages = input
        .iter()
        .filter(|item| item["role"] == "developer")
        .filter_map(|item| item["content"].as_array())
        .flatten()
        .filter_map(|content| content["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(developer_messages.contains(GLOBAL_UPDATE_HEADING));
    assert!(developer_messages.contains(GLOBAL_DELETE_SCOPE));
    assert!(!developer_messages.contains(OLD_UNSCOPED_UPDATE_GATE));

    Ok(())
}

fn create_global_memory_summary(codex_home: &Path) -> std::io::Result<()> {
    let memories_dir = codex_home.join("memories");
    std::fs::create_dir_all(&memories_dir)?;
    std::fs::write(
        memories_dir.join("memory_summary.md"),
        "Global memory summary for scoped policy integration.",
    )
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "{MODEL}"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"

[features]
memories = true

[memories]
generate_memories = false
use_memories = true
use_scoped_memories = true
dedicated_tools = true

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
