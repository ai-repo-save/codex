use std::sync::Arc;

use anyhow::Result;
use codex_context_fragments::ScopedMemoryContextFragment;
use codex_core::config::Config;
use codex_extension_api::ContextContributor;
use codex_extension_api::ContextualUserFragment;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::RewindContextContributionInput;
use codex_protocol::config_types::CollaborationMode;
use codex_protocol::config_types::ModeKind;
use codex_protocol::config_types::Settings;
use codex_protocol::models::ContentItem;
use codex_protocol::models::PermissionProfile;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::RolloutLine;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::TestCodex;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const SAVE_CONTEXT_ANCHOR_TOOL_NAME: &str = "save_context_anchor";
const LIST_CONTEXT_ANCHORS_TOOL_NAME: &str = "list_context_anchors";
const REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME: &str = "rewind_context_to_anchor";
const TEST_MODEL: &str = "gpt-5.4";
const INITIAL_TYPED_CONTEXT: &str = "initial typed context";
const FIRST_REWIND_TYPED_CONTEXT: &str = "first rewind typed context";
const SECOND_REWIND_TYPED_CONTEXT: &str = "second rewind typed context";

struct TypedContextContributor;

impl ContextContributor for TypedContextContributor {
    fn contribute_thread_context_fragments<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        _thread_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<Box<dyn ContextualUserFragment + Send>>> {
        Box::pin(std::future::ready(vec![
            Box::new(ScopedMemoryContextFragment::new(INITIAL_TYPED_CONTEXT))
                as Box<dyn ContextualUserFragment + Send>,
        ]))
    }

    fn contribute_rewind_context_fragments<'a>(
        &'a self,
        _input: RewindContextContributionInput<'a>,
    ) -> ExtensionFuture<'a, Vec<Box<dyn ContextualUserFragment + Send>>> {
        Box::pin(std::future::ready(vec![
            Box::new(ScopedMemoryContextFragment::new(FIRST_REWIND_TYPED_CONTEXT))
                as Box<dyn ContextualUserFragment + Send>,
            Box::new(ScopedMemoryContextFragment::new(
                SECOND_REWIND_TYPED_CONTEXT,
            )) as Box<dyn ContextualUserFragment + Send>,
        ]))
    }
}

fn rewind_typed_context_groups(groups: Vec<Vec<String>>) -> Vec<Vec<String>> {
    groups
        .into_iter()
        .filter(|texts| {
            texts.iter().any(|text| {
                text.contains(FIRST_REWIND_TYPED_CONTEXT)
                    || text.contains(SECOND_REWIND_TYPED_CONTEXT)
            })
        })
        .collect()
}

fn expected_rewind_typed_context_groups() -> Vec<Vec<String>> {
    vec![
        vec![ScopedMemoryContextFragment::new(FIRST_REWIND_TYPED_CONTEXT).render()],
        vec![ScopedMemoryContextFragment::new(SECOND_REWIND_TYPED_CONTEXT).render()],
    ]
}

async fn submit_turn_with_mode(test: &TestCodex, prompt: &str, mode: ModeKind) -> Result<()> {
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.cwd.path());
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: prompt.to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(CollaborationMode {
                    mode,
                    settings: Settings {
                        model: test.session_configured.model.clone(),
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_context_anchors_returns_saved_anchor_metadata() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let save_call_id = "save-anchor-call";
    let list_call_id = "list-anchor-call";
    let mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(
                    save_call_id,
                    SAVE_CONTEXT_ANCHOR_TOOL_NAME,
                    &json!({ "label": "before branch" }).to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_function_call(
                    list_call_id,
                    LIST_CONTEXT_ANCHORS_TOOL_NAME,
                    &json!({ "limit": 5 }).to_string(),
                ),
                ev_completed("resp-2"),
            ]),
            sse(vec![
                ev_response_created("resp-3"),
                ev_assistant_message("msg-1", "done"),
                ev_completed("resp-3"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex();
    let test = builder.build(&server).await?;
    test.submit_turn_with_approval_and_permission_profile(
        "save and list anchors",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let requests = mock.requests();
    assert_eq!(requests.len(), 3);

    let save_text = requests[1]
        .function_call_output_text(save_call_id)
        .expect("save output should be text JSON");
    let save_json: Value = serde_json::from_str(&save_text)?;
    let anchor_id = save_json
        .get("anchor_id")
        .and_then(Value::as_str)
        .expect("save output should include anchor id");

    let list_text = requests[2]
        .function_call_output_text(list_call_id)
        .expect("list output should be text JSON");
    let list_json: Value = serde_json::from_str(&list_text)?;

    assert_eq!(list_json["active_anchor_count"], json!(1));
    assert_eq!(list_json["invalidated_anchor_count"], json!(0));
    assert_eq!(list_json["anchors"][0]["anchor_id"], json!(anchor_id));
    assert_eq!(list_json["anchors"][0]["label"], json!("before branch"));
    assert!(
        list_json["anchors"][0]["response_items_since_anchor"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "anchor listing should include non-zero distance: {list_json:?}"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn successful_context_rewind_replaces_visible_anchor_and_stale_id_is_soft_rejected()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let save_call_id = "save-anchor-call";
    let first_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(
                    save_call_id,
                    SAVE_CONTEXT_ANCHOR_TOOL_NAME,
                    &json!({ "label": "before rewind" }).to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "anchor saved"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut extension_builder = ExtensionRegistryBuilder::<Config>::new();
    extension_builder.prompt_contributor(Arc::new(TypedContextContributor));
    let mut builder = test_codex().with_extensions(Arc::new(extension_builder.build()));
    let test = builder.build(&server).await?;
    test.submit_turn_with_approval_and_permission_profile(
        "save anchor",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let first_requests = first_mock.requests();
    assert_eq!(first_requests.len(), 2);
    assert_eq!(
        first_requests[0]
            .message_input_texts("user")
            .iter()
            .filter(|text| text.contains(INITIAL_TYPED_CONTEXT))
            .count(),
        1
    );

    let save_text = first_requests[1]
        .function_call_output_text(save_call_id)
        .expect("save output should be text JSON");
    let save_json: Value = serde_json::from_str(&save_text)?;
    let anchor_id = save_json
        .get("anchor_id")
        .and_then(Value::as_str)
        .expect("save output should include anchor id");

    let rewind_call_id = "rewind-anchor-call";
    let list_call_id = "list-anchor-call";
    let second_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-3"),
                ev_function_call(
                    rewind_call_id,
                    REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
                    &json!({
                        "anchor_id": anchor_id,
                        "note": "carry forward after successful rewind"
                    })
                    .to_string(),
                ),
                ev_completed("resp-3"),
            ]),
            sse(vec![
                ev_response_created("resp-4"),
                ev_function_call(
                    list_call_id,
                    LIST_CONTEXT_ANCHORS_TOOL_NAME,
                    &json!({ "limit": 10 }).to_string(),
                ),
                ev_completed("resp-4"),
            ]),
            sse(vec![
                ev_response_created("resp-5"),
                ev_assistant_message("msg-2", "done"),
                ev_completed("resp-5"),
            ]),
        ],
    )
    .await;
    test.submit_turn_with_approval_and_permission_profile(
        "rewind then list anchors",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let requests = second_mock.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests[1..] {
        assert_eq!(
            rewind_typed_context_groups(request.message_input_text_groups("user")),
            expected_rewind_typed_context_groups()
        );
    }

    let rewind_text = requests[1]
        .function_call_output_text(rewind_call_id)
        .expect("rewind output should be text JSON");
    let rewind_json: Value = serde_json::from_str(&rewind_text)?;
    let replacement_anchor_id = rewind_json
        .get("replacement_anchor_id")
        .and_then(Value::as_str)
        .expect("rewind output should include replacement anchor id");

    assert_eq!(rewind_json["status"], json!("rewound"));
    assert_eq!(rewind_json["anchor_id"], json!(anchor_id));

    let list_text = requests[2]
        .function_call_output_text(list_call_id)
        .expect("list output should be text JSON");
    let list_json: Value = serde_json::from_str(&list_text)?;

    assert_eq!(list_json["active_anchor_count"], json!(1));
    assert_eq!(
        list_json["anchors"][0]["anchor_id"],
        json!(replacement_anchor_id)
    );
    let current_history_items = list_json["current_history_items"]
        .as_u64()
        .expect("list output should include current history size");
    let replacement_history_boundary = list_json["anchors"][0]["history_boundary"]
        .as_u64()
        .expect("replacement anchor should include its history boundary");
    let response_items_since_anchor = list_json["anchors"][0]["response_items_since_anchor"]
        .as_u64()
        .expect("replacement anchor should include its response distance");
    assert_eq!(
        replacement_history_boundary + response_items_since_anchor,
        current_history_items
    );

    let stale_rewind_call_id = "stale-rewind-anchor-call";
    let third_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-6"),
                ev_function_call(
                    stale_rewind_call_id,
                    REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
                    &json!({
                        "anchor_id": anchor_id,
                        "note": "stale anchor should not interrupt this turn"
                    })
                    .to_string(),
                ),
                ev_completed("resp-6"),
            ]),
            sse(vec![
                ev_response_created("resp-7"),
                ev_assistant_message("msg-3", "continued after stale anchor rejection"),
                ev_completed("resp-7"),
            ]),
        ],
    )
    .await;
    test.submit_turn_with_approval_and_permission_profile(
        "reuse consumed anchor",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let requests = third_mock.requests();
    assert_eq!(requests.len(), 2);
    let stale_rewind_text = requests[1]
        .function_call_output_text(stale_rewind_call_id)
        .expect("stale rewind output should be text JSON");
    let stale_rewind_json: Value = serde_json::from_str(&stale_rewind_text)?;

    assert_eq!(
        stale_rewind_json,
        json!({
            "status": "rejected",
            "anchor_id": anchor_id,
            "replacement_anchor_id": replacement_anchor_id,
            "reason": "unknown_context_anchor",
        })
    );

    let rollout_path = test.codex.rollout_path().expect("rollout path");
    let rollout = tokio::fs::read_to_string(rollout_path).await?;
    let persisted_rewind_context_groups = rollout
        .lines()
        .map(serde_json::from_str::<RolloutLine>)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .filter_map(|line| match line.item {
            RolloutItem::ResponseItem(ResponseItem::Message { content, .. }) => Some(content),
            _ => None,
        })
        .map(|content| {
            content
                .into_iter()
                .filter_map(|item| match item {
                    ContentItem::InputText { text } => Some(text),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rewind_typed_context_groups(persisted_rewind_context_groups),
        expected_rewind_typed_context_groups()
    );

    let resume_mock = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp-8"), ev_completed("resp-8")]),
    )
    .await;
    let rollout_path = test
        .session_configured
        .rollout_path
        .clone()
        .expect("rollout path");
    let resumed = builder
        .resume(&server, test.home.clone(), rollout_path)
        .await?;
    resumed
        .submit_turn_with_approval_and_permission_profile(
            "continue after rewind resume",
            AskForApproval::Never,
            PermissionProfile::Disabled,
        )
        .await?;
    assert_eq!(
        rewind_typed_context_groups(
            resume_mock
                .single_request()
                .message_input_text_groups("user")
        ),
        expected_rewind_typed_context_groups()
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn low_benefit_context_rewind_returns_rejected_output_without_ending_turn() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let save_call_id = "save-anchor-call";
    let first_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(
                    save_call_id,
                    SAVE_CONTEXT_ANCHOR_TOOL_NAME,
                    &json!({ "label": "before low benefit" }).to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "anchor saved"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex()
        .with_model_info_override(TEST_MODEL, |model_info| {
            model_info.context_window = Some(1_000);
            model_info.effective_context_window_percent = 100;
        })
        .with_config(|config| {
            config.context_rewind.min_reclaim_percent = 100;
        });
    let test = builder.build(&server).await?;
    test.submit_turn_with_approval_and_permission_profile(
        "save anchor",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let first_requests = first_mock.requests();
    assert_eq!(first_requests.len(), 2);

    let save_text = first_requests[1]
        .function_call_output_text(save_call_id)
        .expect("save output should be text JSON");
    let save_json: Value = serde_json::from_str(&save_text)?;
    let anchor_id = save_json
        .get("anchor_id")
        .and_then(Value::as_str)
        .expect("save output should include anchor id");

    let rewind_call_id = "rewind-anchor-call";
    let list_call_id = "list-anchor-call";
    let second_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-3"),
                ev_function_call(
                    rewind_call_id,
                    REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
                    &json!({
                        "anchor_id": anchor_id,
                        "note": "carry forward only if allowed"
                    })
                    .to_string(),
                ),
                ev_completed("resp-3"),
            ]),
            sse(vec![
                ev_response_created("resp-4"),
                ev_function_call(
                    list_call_id,
                    LIST_CONTEXT_ANCHORS_TOOL_NAME,
                    &json!({ "limit": 10 }).to_string(),
                ),
                ev_completed("resp-4"),
            ]),
            sse(vec![
                ev_response_created("resp-5"),
                ev_assistant_message("msg-2", "continued after rejection"),
                ev_completed("resp-5"),
            ]),
        ],
    )
    .await;
    test.submit_turn_with_approval_and_permission_profile(
        "try low benefit rewind",
        AskForApproval::Never,
        PermissionProfile::Disabled,
    )
    .await?;

    let requests = second_mock.requests();
    assert_eq!(requests.len(), 3);

    let rewind_text = requests[1]
        .function_call_output_text(rewind_call_id)
        .expect("rewind output should be text JSON");
    let rewind_json: Value = serde_json::from_str(&rewind_text)?;

    assert_eq!(rewind_json["status"], json!("rejected"));
    assert_eq!(rewind_json["reason"], json!("below_min_reclaim_percent"));
    assert_eq!(rewind_json["anchor_id"], json!(anchor_id));
    assert_eq!(rewind_json["min_reclaim_percent"], json!(100));
    assert_eq!(rewind_json["min_reclaim_threshold_tokens"], json!(1_000));
    assert_eq!(rewind_json["model_context_window"], json!(1_000));

    let list_text = requests[2]
        .function_call_output_text(list_call_id)
        .expect("list output should be text JSON");
    let list_json: Value = serde_json::from_str(&list_text)?;

    assert_eq!(list_json["active_anchor_count"], json!(1));
    assert_eq!(list_json["invalidated_anchor_count"], json!(0));
    assert_eq!(list_json["anchors"][0]["anchor_id"], json!(anchor_id));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incompatible_mode_context_rewind_returns_rejected_output_without_consuming_anchor()
-> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let save_call_id = "save-plan-anchor-call";
    let first_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-1"),
                ev_function_call(
                    save_call_id,
                    SAVE_CONTEXT_ANCHOR_TOOL_NAME,
                    &json!({ "label": "before mode transition" }).to_string(),
                ),
                ev_completed("resp-1"),
            ]),
            sse(vec![
                ev_response_created("resp-2"),
                ev_assistant_message("msg-1", "anchor saved in plan mode"),
                ev_completed("resp-2"),
            ]),
        ],
    )
    .await;

    let mut builder = test_codex();
    let test = builder.build(&server).await?;
    submit_turn_with_mode(&test, "save a plan mode anchor", ModeKind::Plan).await?;

    let first_requests = first_mock.requests();
    assert_eq!(first_requests.len(), 2);
    let save_text = first_requests[1]
        .function_call_output_text(save_call_id)
        .expect("save output should be text JSON");
    let save_json: Value = serde_json::from_str(&save_text)?;
    let anchor_id = save_json
        .get("anchor_id")
        .and_then(Value::as_str)
        .expect("save output should include anchor id");

    let rewind_call_id = "rewind-default-mode-call";
    let list_call_id = "list-after-mode-rejection-call";
    let second_mock = mount_sse_sequence(
        &server,
        vec![
            sse(vec![
                ev_response_created("resp-3"),
                ev_function_call(
                    rewind_call_id,
                    REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
                    &json!({
                        "anchor_id": anchor_id,
                        "note": "try to rewind after switching modes"
                    })
                    .to_string(),
                ),
                ev_completed("resp-3"),
            ]),
            sse(vec![
                ev_response_created("resp-4"),
                ev_function_call(
                    list_call_id,
                    LIST_CONTEXT_ANCHORS_TOOL_NAME,
                    &json!({ "limit": 10 }).to_string(),
                ),
                ev_completed("resp-4"),
            ]),
            sse(vec![
                ev_response_created("resp-5"),
                ev_assistant_message("msg-2", "continued after mode rejection"),
                ev_completed("resp-5"),
            ]),
        ],
    )
    .await;
    submit_turn_with_mode(
        &test,
        "rewind after switching to default mode",
        ModeKind::Default,
    )
    .await?;

    let requests = second_mock.requests();
    assert_eq!(requests.len(), 3);
    let rewind_text = requests[1]
        .function_call_output_text(rewind_call_id)
        .expect("rewind output should contain text JSON");
    let rewind_json: Value = serde_json::from_str(&rewind_text)?;
    assert_eq!(
        rewind_json,
        json!({
            "status": "rejected",
            "anchor_id": anchor_id,
            "reason": "incompatible_collaboration_mode",
            "anchor_collaboration_mode": "plan",
            "current_collaboration_mode": "default",
        })
    );

    let list_text = requests[2]
        .function_call_output_text(list_call_id)
        .expect("list output should be text JSON");
    let list_json: Value = serde_json::from_str(&list_text)?;
    assert_eq!(list_json["active_anchor_count"], json!(1));
    assert_eq!(list_json["invalidated_anchor_count"], json!(0));
    assert_eq!(list_json["anchors"][0]["anchor_id"], json!(anchor_id));

    Ok(())
}
