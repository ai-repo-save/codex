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
