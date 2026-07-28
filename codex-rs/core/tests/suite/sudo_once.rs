#![allow(clippy::unwrap_used)]

use anyhow::Result;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::SessionSource;
use codex_protocol::sudo_once::SudoOnceApprovalDecision;
use codex_protocol::sudo_once::SudoOnceApprovalResponse;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::local;
use core_test_support::test_codex::local_selections;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;

fn exec_command_exposes_sudo_once(body: &serde_json::Value) -> bool {
    body.get("tools")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|tool| {
            tool.get("name").and_then(serde_json::Value::as_str) == Some("exec_command")
        })
        .and_then(|tool| tool.get("parameters"))
        .and_then(|parameters| parameters.get("properties"))
        .and_then(|properties| properties.get("privilege"))
        .is_some()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sudo_once_is_not_exposed_to_exec_sessions() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let response_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-sudo-once-exec-source-1"),
            ev_assistant_message("msg-sudo-once-exec-source-1", "done"),
            ev_completed("resp-sudo-once-exec-source-1"),
        ]),
    )
    .await;
    let mut builder = test_codex().with_config(|config| {
        config.use_experimental_unified_exec_tool = true;
        config
            .features
            .enable(Feature::UnifiedExec)
            .expect("test config should allow feature update");
        config
            .features
            .enable(Feature::SudoOnce)
            .expect("test config should allow feature update");
    });
    let test = builder.build(&server).await?;

    test.submit_turn_with_environments(
        "show tools",
        Some(vec![local(test.config.cwd.clone())]),
    )
    .await?;

    assert!(
        !exec_command_exposes_sudo_once(&response_mock.single_request().body_json()),
        "sudo_once must remain unavailable to exec sessions"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sudo_once_requests_dedicated_approval_when_general_approval_is_never() -> Result<()> {
    let server = start_mock_server().await;
    let mut builder = test_codex()
        .with_session_source(SessionSource::Cli)
        .with_config(|config| {
            config.use_experimental_unified_exec_tool = true;
            config
                .features
                .enable(Feature::UnifiedExec)
                .expect("test config should allow feature update");
            config
                .features
                .enable(Feature::SudoOnce)
                .expect("test config should allow feature update");
        });
    let test = builder.build(&server).await?;
    let call_id = "sudo-once-requires-dedicated-approval";
    let command = "touch sudo-once-must-not-run";
    let args = json!({
        "cmd": command,
        "shell": "/bin/sh",
        "login": false,
        "tty": false,
        "privilege": "sudo_once",
        "justification": "verify the dedicated sudo approval path",
    });
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-sudo-once-1"),
            ev_function_call(call_id, "exec_command", &serde_json::to_string(&args)?),
            ev_completed("resp-sudo-once-1"),
        ]),
    )
    .await;

    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "request one privileged command".into(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(local_selections(test.config.cwd.clone())),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;

    let approval = wait_for_event(&test.codex, |event| {
        matches!(
            event,
            EventMsg::SudoOnceApprovalRequest(_) | EventMsg::TurnComplete(_) | EventMsg::Error(_)
        )
    })
    .await;
    let EventMsg::SudoOnceApprovalRequest(approval) = approval else {
        panic!("sudo_once must request dedicated approval despite approval_policy=never: {approval:?}");
    };
    assert_eq!(approval.call_id, call_id);
    assert_eq!(approval.command, vec!["/bin/sh", "-c", command]);
    assert_eq!(approval.cwd, test.config.cwd);
    assert_eq!(
        approval.reason.as_deref(),
        Some("verify the dedicated sudo approval path")
    );

    test.codex
        .submit(Op::SudoOnceApproval {
            id: approval.call_id,
            turn_id: Some(approval.turn_id),
            response: SudoOnceApprovalResponse {
                decision: SudoOnceApprovalDecision::Abort,
            },
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert!(
        !test.workspace_path("sudo-once-must-not-run").exists(),
        "an aborted sudo_once request must not launch its command"
    );

    Ok(())
}
