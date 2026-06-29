use anyhow::Result;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_sequence;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const SAVE_CONTEXT_ANCHOR_TOOL_NAME: &str = "save_context_anchor";
const LIST_CONTEXT_ANCHORS_TOOL_NAME: &str = "list_context_anchors";
const REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME: &str = "rewind_context_to_anchor";
const TEST_MODEL: &str = "gpt-5.4";

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
                ev_assistant_message("msg-2", "continued after rejection"),
                ev_completed("resp-4"),
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
    assert_eq!(requests.len(), 2);

    let second_request = requests[0].body_json();
    let rewind_arguments = second_request["input"]
        .as_array()
        .expect("request input should be an array")
        .iter()
        .find(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call")
                && item.get("call_id").and_then(Value::as_str) == Some(rewind_call_id)
        })
        .and_then(|item| item.get("arguments"))
        .and_then(Value::as_str)
        .expect("rewind call should have arguments");
    assert_eq!(
        serde_json::from_str::<Value>(rewind_arguments)?["anchor_id"],
        json!(anchor_id)
    );

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

    Ok(())
}
