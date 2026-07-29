use super::*;
use crate::session::step_context::StepContext;
use crate::session::tests::make_session_and_context_with_rx;
use crate::tools::context::ToolCallSource;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolPayload;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_protocol::items::SkillLoadItem;
use codex_protocol::items::SkillLoadStatus;
use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SkillScope;
use codex_utils_absolute_path::AbsolutePathBuf;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::HashSet;
use tokio::sync::Mutex;

fn skill_metadata(name: &str, path: AbsolutePathBuf) -> codex_core_skills::SkillMetadata {
    codex_core_skills::SkillMetadata {
        name: name.to_string(),
        description: format!("{name} description"),
        short_description: None,
        interface: None,
        dependencies: None,
        policy: None,
        path_to_skills_md: path,
        scope: SkillScope::Repo,
        plugin_id: None,
        remote_plugin_id: None,
    }
}

fn skill_outcome(
    skills: Vec<codex_core_skills::SkillMetadata>,
    disabled_paths: HashSet<AbsolutePathBuf>,
) -> Arc<SkillLoadOutcome> {
    let mut outcome = SkillLoadOutcome::default();
    outcome.skills = skills;
    outcome.disabled_paths = disabled_paths;
    Arc::new(outcome)
}

async fn invocation_with_rx(name: &str) -> (ToolInvocation, async_channel::Receiver<Event>) {
    let (session, turn, rx) = make_session_and_context_with_rx().await;
    let step_context = StepContext::for_test(Arc::clone(&turn));
    (
        ToolInvocation {
            session,
            step_context,
            turn,
            cancellation_token: tokio_util::sync::CancellationToken::new(),
            tracker: Arc::new(Mutex::new(TurnDiffTracker::default())),
            call_id: "call-use-skill".to_string(),
            tool_name: ToolName::plain(USE_SKILL_TOOL_NAME),
            source: ToolCallSource::Direct,
            payload: ToolPayload::Function {
                arguments: json!({ "name": name }).to_string(),
            },
        },
        rx,
    )
}

async fn invocation(name: &str) -> ToolInvocation {
    let (invocation, _rx) = invocation_with_rx(name).await;
    invocation
}

async fn next_skill_load_item(rx: &async_channel::Receiver<Event>) -> SkillLoadItem {
    let event = rx.recv().await.expect("skill load item event");
    let EventMsg::ItemCompleted(completed) = event.msg else {
        panic!("expected completed skill load item");
    };
    let codex_protocol::items::TurnItem::SkillLoad(item) = completed.item else {
        panic!("expected skill load item");
    };
    item
}

fn output_text(output: Box<dyn crate::tools::context::ToolOutput>) -> String {
    let response = output.to_response_item(
        "call-use-skill",
        &ToolPayload::Function {
            arguments: "{}".to_string(),
        },
    );
    let ResponseInputItem::FunctionCallOutput {
        output: FunctionCallOutputPayload { body, .. },
        ..
    } = response
    else {
        panic!("expected function call output content");
    };
    body.to_text().expect("text output")
}

fn expect_respond_to_model(
    result: Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError>,
) -> String {
    match result {
        Ok(_) => panic!("tool call should fail"),
        Err(FunctionCallError::RespondToModel(message)) => message,
        Err(err) => panic!("unexpected fatal error: {err}"),
    }
}

#[tokio::test]
async fn use_skill_loads_enabled_skill_body_without_frontmatter() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let skill_path =
        AbsolutePathBuf::try_from(tempdir.path().join("SKILL.md")).expect("absolute skill path");
    std::fs::write(
        skill_path.as_path(),
        "---\nname: demo\ndescription: demo skill\n---\n\n# Demo\n\nUse it.\n",
    )
    .expect("write skill");
    let handler = UseSkillHandler::new(skill_outcome(
        vec![skill_metadata("demo", skill_path.clone())],
        HashSet::new(),
    ));

    let (invocation, rx) = invocation_with_rx("demo").await;
    let output = handler
        .handle(invocation)
        .await
        .expect("use_skill should load the skill");

    assert_eq!(
        output_text(output),
        format!(
            "\n<name>demo</name>\n<path>{}</path>\n# Demo\n\nUse it.\n\n",
            skill_path.display()
        )
    );
    assert_eq!(
        next_skill_load_item(&rx).await,
        SkillLoadItem {
            id: "call-use-skill".to_string(),
            name: "demo".to_string(),
            path: Some(skill_path),
            status: SkillLoadStatus::Completed,
            error: None,
        }
    );
}

#[tokio::test]
async fn use_skill_rejects_unknown_skill_name() {
    let handler = UseSkillHandler::new(skill_outcome(Vec::new(), HashSet::new()));
    let (invocation, rx) = invocation_with_rx("missing").await;

    let message = expect_respond_to_model(handler.handle(invocation).await);

    assert_eq!(
        message,
        "skill `missing` was not found in the available skills list"
    );
    assert_eq!(
        next_skill_load_item(&rx).await,
        SkillLoadItem {
            id: "call-use-skill".to_string(),
            name: "missing".to_string(),
            path: None,
            status: SkillLoadStatus::Failed,
            error: Some("skill `missing` was not found in the available skills list".to_string()),
        }
    );
}

#[tokio::test]
async fn use_skill_rejects_disabled_skill_name() {
    let skill_path = AbsolutePathBuf::try_from(std::env::temp_dir().join("disabled/SKILL.md"))
        .expect("absolute skill path");
    let handler = UseSkillHandler::new(skill_outcome(
        vec![skill_metadata("demo", skill_path.clone())],
        HashSet::from([skill_path]),
    ));

    let message = expect_respond_to_model(handler.handle(invocation("demo").await).await);

    assert_eq!(message, "skill `demo` is disabled");
}

#[tokio::test]
async fn use_skill_rejects_duplicate_enabled_names() {
    let first = AbsolutePathBuf::try_from(std::env::temp_dir().join("one/SKILL.md"))
        .expect("absolute skill path");
    let second = AbsolutePathBuf::try_from(std::env::temp_dir().join("two/SKILL.md"))
        .expect("absolute skill path");
    let handler = UseSkillHandler::new(skill_outcome(
        vec![
            skill_metadata("demo", first.clone()),
            skill_metadata("demo", second.clone()),
        ],
        HashSet::new(),
    ));

    let message = expect_respond_to_model(handler.handle(invocation("demo").await).await);

    assert_eq!(
        message,
        format!(
            "skill name `demo` is ambiguous; matching SKILL.md paths: {}, {}",
            first.display(),
            second.display()
        )
    );
}
