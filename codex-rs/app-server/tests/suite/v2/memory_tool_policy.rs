use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::JSONRPCResponse;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use pretty_assertions::assert_eq;
use serde_json::Value;
use tempfile::TempDir;
use tokio::time::timeout;

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const MEMORIES_NAMESPACE: &str = "memories";
const WRITE_NOTE_TOOL: &str = "write_note";
const DELETE_TOOL: &str = "delete";
const SESSION_PROACTIVE_POLICY: &str = "no explicit user request is required";
const PROJECT_POLICY_AUTHORITY: &str = "project AGENTS.md instructions authorize";
const GLOBAL_EXPLICIT_POLICY: &str = "only when the user explicitly asks Codex";
const OLD_SCOPED_EXPLICIT_POLICY: &str =
    "after the user explicitly asks Codex to remember, forget, or update something for this session or project";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scoped_memory_policy_reaches_responses_lite_as_developer_tools() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let responses_server = responses::start_mock_server().await;
    let response_mock = responses::mount_sse_once(
        &responses_server,
        responses::sse(vec![
            responses::ev_response_created("resp-1"),
            responses::ev_completed("resp-1"),
        ]),
    )
    .await;
    let codex_home = TempDir::new()?;
    create_config_toml(codex_home.path(), &responses_server.uri())?;

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

    let request_body = response_mock.single_request().body_json();
    assert!(request_body.get("tools").is_none());
    let input = request_body["input"]
        .as_array()
        .context("Responses input should be an array")?;
    let additional_tools = input
        .first()
        .context("Responses input should include additional tools")?;
    assert_eq!(additional_tools["type"], "additional_tools");
    assert_eq!(additional_tools["role"], "developer");

    let namespace = additional_tools["tools"]
        .as_array()
        .context("additional_tools.tools should be an array")?
        .iter()
        .find(|tool| {
            tool["type"] == "namespace" && tool["name"].as_str() == Some(MEMORIES_NAMESPACE)
        })
        .context("Responses Lite should include the memories namespace")?;
    let memory_tools = namespace["tools"]
        .as_array()
        .context("memories namespace tools should be an array")?;
    let write_description = tool_description(memory_tools, WRITE_NOTE_TOOL)?;
    let delete_description = tool_description(memory_tools, DELETE_TOOL)?;

    assert!(write_description.contains(SESSION_PROACTIVE_POLICY));
    assert!(write_description.contains(PROJECT_POLICY_AUTHORITY));
    assert!(delete_description.contains(SESSION_PROACTIVE_POLICY));
    assert!(delete_description.contains(PROJECT_POLICY_AUTHORITY));
    assert!(delete_description.contains(GLOBAL_EXPLICIT_POLICY));
    assert!(!write_description.contains(OLD_SCOPED_EXPLICIT_POLICY));
    assert!(!delete_description.contains(OLD_SCOPED_EXPLICIT_POLICY));

    Ok(())
}

fn tool_description<'a>(tools: &'a [Value], name: &str) -> Result<&'a str> {
    tools
        .iter()
        .find(|tool| tool["name"].as_str() == Some(name))
        .with_context(|| format!("memories namespace should include {name}"))?["description"]
        .as_str()
        .with_context(|| format!("{name} should include a description"))
}

fn create_config_toml(codex_home: &Path, server_uri: &str) -> std::io::Result<()> {
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            r#"
model = "gpt-5.6-terra"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"

[features]
memories = true

[memories]
generate_memories = false
use_memories = false
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
