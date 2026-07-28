use anyhow::Result;
use codex_features::Feature;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use codex_sudo_once::LocalSudoOnceBroker;
use codex_sudo_once::SudoOncePrompt;
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
use serde_json::Value;
use serde_json::json;

const COMMAND: &str = "touch sudo-once-must-not-run";
const JUSTIFICATION: &str = "verify the local sudo approval boundary";
const MARKER: &str = "sudo-once-must-not-run";

fn configure_sudo_once(config: &mut codex_core::config::Config) {
    config.use_experimental_unified_exec_tool = true;
    config
        .features
        .enable(Feature::UnifiedExec)
        .expect("test config should enable unified exec");
    config
        .features
        .enable(Feature::SudoOnce)
        .expect("test config should enable sudo once");
}

fn exec_command_privilege(body: &Value) -> Option<&Value> {
    body.get("tools")?
        .as_array()?
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("exec_command"))?
        .get("parameters")?
        .get("properties")?
        .get("privilege")
}

async fn collect_exec_command_privilege(
    broker: Option<LocalSudoOnceBroker>,
) -> Result<Option<Value>> {
    let server = start_mock_server().await;
    let response_mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-sudo-once-tool-schema"),
            ev_assistant_message("msg-sudo-once-tool-schema", "done"),
            ev_completed("resp-sudo-once-tool-schema"),
        ]),
    )
    .await;
    let mut builder = test_codex().with_config(configure_sudo_once);
    if let Some(broker) = broker {
        builder = builder.with_sudo_once_broker(broker);
    }
    let test = builder.build(&server).await?;

    test.submit_turn_with_environments("show tools", Some(vec![local(test.config.cwd.clone())]))
        .await?;

    Ok(exec_command_privilege(&response_mock.single_request().body_json()).cloned())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sudo_once_schema_requires_a_local_broker() -> Result<()> {
    skip_if_no_network!(Ok(()));

    assert_eq!(collect_exec_command_privilege(None).await?, None);

    let (broker, _prompts) = LocalSudoOnceBroker::new();
    let privilege = collect_exec_command_privilege(Some(broker)).await?;
    assert_eq!(
        privilege.as_ref().and_then(|value| value.get("type")),
        Some(&json!("string"))
    );
    assert_eq!(
        privilege.as_ref().and_then(|value| value.get("enum")),
        Some(&json!(["sudo_once"]))
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sudo_once_broker_prompts_despite_general_approval_never_and_abort_finishes_turn()
-> Result<()> {
    let server = start_mock_server().await;
    let (broker, mut prompts) = LocalSudoOnceBroker::new();
    let mut builder = test_codex()
        .with_sudo_once_broker(broker)
        .with_config(configure_sudo_once);
    let test = builder.build(&server).await?;
    let args = json!({
        "cmd": COMMAND,
        "shell": "/bin/sh",
        "login": false,
        "tty": false,
        "privilege": "sudo_once",
        "justification": JUSTIFICATION,
    });
    mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-sudo-once-abort"),
            ev_function_call(
                "sudo-once-abort",
                "exec_command",
                &serde_json::to_string(&args)?,
            ),
            ev_completed("resp-sudo-once-abort"),
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

    let Some(SudoOncePrompt::Approval(prompt)) = prompts.recv().await else {
        panic!("sudo_once must request an approval prompt");
    };
    let (command, responder) = prompt.into_parts();
    assert_eq!(command.argv(), ["/bin/sh", "-c", COMMAND]);
    assert_eq!(command.cwd(), &test.config.cwd);
    assert_eq!(command.reason(), Some(JUSTIFICATION));
    assert!(responder.abort());

    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert!(!test.workspace_path(MARKER).exists());

    Ok(())
}
