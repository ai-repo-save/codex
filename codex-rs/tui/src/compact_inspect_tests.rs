use std::fs;

use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::RolloutItem;
use pretty_assertions::assert_eq;

use super::CompactInspectError;
use super::read_latest_compaction;
use super::write_latest_compaction_report;

#[test]
fn read_latest_compaction_selects_last_valid_compaction_record() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let rollout_path = temp_dir.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        [
            "not json".to_string(),
            rollout_line(RolloutItem::ResponseItem(message(
                "user",
                "ordinary user message",
            ))),
            rollout_line(RolloutItem::Compacted(compacted(
                "first summary",
                Some(vec![message("user", "first retained message")]),
            ))),
            "{".to_string(),
            rollout_line(RolloutItem::Compacted(compacted(
                "latest summary",
                Some(vec![
                    message("user", "latest retained user message"),
                    message("assistant", "latest retained assistant message"),
                ]),
            ))),
        ]
        .join("\n"),
    )
    .expect("write rollout");

    let latest = read_latest_compaction(&rollout_path).expect("latest compaction");

    assert_eq!(latest.line_number, 5);
    assert_eq!(latest.compacted.message, "latest summary");
    assert_eq!(
        latest
            .compacted
            .replacement_history
            .as_ref()
            .expect("replacement history")
            .len(),
        2
    );
}

#[test]
fn read_latest_compaction_reports_no_compaction_record() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let rollout_path = temp_dir.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        rollout_line(RolloutItem::ResponseItem(message(
            "user",
            "ordinary user message",
        ))),
    )
    .expect("write rollout");

    let err = read_latest_compaction(&rollout_path).expect_err("expected no compaction");

    assert!(matches!(err, CompactInspectError::NoCompaction));
}

#[test]
fn write_latest_compaction_report_preserves_absent_replacement_history() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let rollout_path = temp_dir.path().join("rollout.jsonl");
    fs::write(
        &rollout_path,
        rollout_line(RolloutItem::Compacted(compacted("summary only", None))),
    )
    .expect("write rollout");
    let latest = read_latest_compaction(&rollout_path).expect("latest compaction");

    let report_path =
        write_latest_compaction_report(&latest, /*thread_id*/ None).expect("write report");
    let report = fs::read_to_string(report_path).expect("read report");
    let parsed = serde_json::from_str::<serde_json::Value>(&report).expect("report json");

    assert_eq!(parsed["lineNumber"], 1);
    assert_eq!(parsed["message"], "summary only");
    assert_eq!(parsed["replacementHistoryItemCount"], 0);
    assert_eq!(parsed["replacementHistory"], serde_json::Value::Null);
}

fn compacted(message: &str, replacement_history: Option<Vec<ResponseItem>>) -> CompactedItem {
    CompactedItem {
        message: message.to_string(),
        replacement_history,
        window_id: None,
    }
}

fn message(role: &str, text: &str) -> ResponseItem {
    ResponseItem::Message {
        id: None,
        role: role.to_string(),
        content: vec![ContentItem::InputText {
            text: text.to_string(),
        }],
        phase: None,
        metadata: None,
    }
}

fn rollout_line(item: RolloutItem) -> String {
    serde_json::to_string(&item).expect("rollout item json")
}
