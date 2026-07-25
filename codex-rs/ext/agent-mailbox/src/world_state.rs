use codex_extension_api::PreviousWorldStateSection;
use codex_extension_api::RenderedWorldStateFragment;
use codex_extension_api::WorldStateSectionContribution;
use codex_state::AgentMailboxUnreadSnapshot;
use serde_json::json;

const WORLD_STATE_ID: &str = "agent_mailbox";
const OPEN_MARKER: &str = "<agent_mailbox>";
const CLOSE_MARKER: &str = "</agent_mailbox>";

pub(crate) fn mailbox_world_state_section(
    snapshot: AgentMailboxUnreadSnapshot,
) -> WorldStateSectionContribution {
    let rendered_body = render_snapshot(&snapshot);
    let comparison = json!({
        "total": snapshot.total,
        "progress": snapshot.progress,
        "result": snapshot.result,
        "actionRequired": snapshot.action_required,
        "revision": snapshot.revision,
    });
    let retained_body = rendered_body.clone();

    WorldStateSectionContribution::new(WORLD_STATE_ID, comparison.clone(), move |previous| {
        if matches!(
            previous,
            PreviousWorldStateSection::Known(previous) if previous == &comparison
        ) {
            return None;
        }
        if snapshot.total == 0 && matches!(previous, PreviousWorldStateSection::Absent) {
            return None;
        }
        Some(RenderedWorldStateFragment::new(
            "developer",
            (OPEN_MARKER, CLOSE_MARKER),
            rendered_body.clone(),
        ))
    })
    .with_legacy_matcher(|role, text| {
        role == "developer"
            && text.trim_start().starts_with(OPEN_MARKER)
            && text.trim_end().ends_with(CLOSE_MARKER)
    })
    .with_retained_fragment_matcher(move |role, text| {
        role == "developer" && text.contains(&retained_body)
    })
}

fn render_snapshot(snapshot: &AgentMailboxUnreadSnapshot) -> String {
    if snapshot.total == 0 {
        return "Agent mailbox: 0 unread.".to_string();
    }

    let mut categories = Vec::new();
    if snapshot.action_required > 0 {
        categories.push(format!("{} action required", snapshot.action_required));
    }
    if snapshot.result > 0 {
        categories.push(format!("{} result", snapshot.result));
    }
    if snapshot.progress > 0 {
        categories.push(format!("{} progress", snapshot.progress));
    }
    format!(
        "Agent mailbox: {} unread — {}. Use agent_mailbox.read to process them.",
        snapshot.total,
        categories.join(", ")
    )
}
