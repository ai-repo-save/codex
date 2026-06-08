use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::CompactedItem;
use codex_protocol::protocol::RolloutItem;
use serde::Serialize;

pub(crate) const COMPACT_INSPECT_MISSING_ROLLOUT: &str = "Rollout path is not available yet.";
pub(crate) const COMPACT_INSPECT_NO_COMPACTION: &str =
    "No compaction records found in the current rollout.";
pub(crate) const COMPACT_INSPECT_OUTPUT_DIR: &str = "/tmp/codex-compact-inspect";
pub(crate) const COMPACT_INSPECT_RESULT_KIND: &str =
    "installed replacement history / post-processed compaction result";
pub(crate) const COMPACT_INSPECT_REMOTE_NOTE: &str =
    "This is the rollout-persisted result, not necessarily the raw remote compaction response.";

const PREVIEW_CHAR_LIMIT: usize = 1_200;

#[derive(Debug, Clone)]
pub(crate) struct LatestCompaction {
    pub(crate) rollout_path: PathBuf,
    pub(crate) line_number: usize,
    pub(crate) compacted: CompactedItem,
}

#[derive(Debug)]
pub(crate) enum CompactInspectError {
    Read(io::Error),
    NoCompaction,
    Serialize(serde_json::Error),
    Write(io::Error),
}

impl std::fmt::Display for CompactInspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(err) => write!(f, "Failed to read rollout file: {err}"),
            Self::NoCompaction => write!(f, "{COMPACT_INSPECT_NO_COMPACTION}"),
            Self::Serialize(err) => write!(f, "Failed to serialize compaction report: {err}"),
            Self::Write(err) => write!(f, "Failed to write compaction report: {err}"),
        }
    }
}

impl std::error::Error for CompactInspectError {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompactInspectJson<'a> {
    rollout_path: String,
    line_number: usize,
    result_kind: &'static str,
    remote_response_note: &'static str,
    message: &'a str,
    replacement_history_item_count: usize,
    replacement_history: &'a Option<Vec<ResponseItem>>,
}

pub(crate) fn read_latest_compaction(
    rollout_path: &Path,
) -> Result<LatestCompaction, CompactInspectError> {
    let contents = fs::read_to_string(rollout_path).map_err(CompactInspectError::Read)?;
    let mut latest = None;
    for (index, line) in contents.lines().enumerate() {
        let Ok(RolloutItem::Compacted(compacted)) = serde_json::from_str::<RolloutItem>(line)
        else {
            continue;
        };
        latest = Some(LatestCompaction {
            rollout_path: rollout_path.to_path_buf(),
            line_number: index + 1,
            compacted,
        });
    }
    latest.ok_or(CompactInspectError::NoCompaction)
}

pub(crate) fn write_latest_compaction_report(
    compaction: &LatestCompaction,
    thread_id: Option<ThreadId>,
) -> Result<PathBuf, CompactInspectError> {
    let output_dir = PathBuf::from(COMPACT_INSPECT_OUTPUT_DIR);
    fs::create_dir_all(&output_dir).map_err(CompactInspectError::Write)?;
    let output_path = output_dir.join(format!(
        "{}-latest.json",
        report_file_stem(&compaction.rollout_path, thread_id.as_ref())
    ));
    let report = compact_inspect_json(compaction);
    let json = serde_json::to_string_pretty(&report).map_err(CompactInspectError::Serialize)?;
    fs::write(&output_path, format!("{json}\n")).map_err(CompactInspectError::Write)?;
    Ok(output_path)
}

pub(crate) fn render_compaction_summary(
    compaction: &LatestCompaction,
    output_path: &Path,
) -> String {
    let replacement_history_item_count = compaction
        .compacted
        .replacement_history
        .as_ref()
        .map_or(0, Vec::len);
    let replacement_history_preview = compaction
        .compacted
        .replacement_history
        .as_ref()
        .map(|items| preview_json(items))
        .unwrap_or_else(|| {
            "replacement_history is not present in this compaction record.".to_string()
        });
    format!(
        "Latest compaction result\n\
         Kind: {COMPACT_INSPECT_RESULT_KIND}\n\
         Note: {COMPACT_INSPECT_REMOTE_NOTE}\n\
         Rollout: {rollout_path}\n\
         Line: {line_number}\n\
         Message: {message}\n\
         Replacement history items: {replacement_history_item_count}\n\
         Full JSON: {output_path}\n\
         Preview:\n{replacement_history_preview}",
        rollout_path = compaction.rollout_path.display(),
        line_number = compaction.line_number,
        message = compaction.compacted.message.as_str(),
        output_path = output_path.display(),
    )
}

fn compact_inspect_json(compaction: &LatestCompaction) -> CompactInspectJson<'_> {
    CompactInspectJson {
        rollout_path: compaction.rollout_path.display().to_string(),
        line_number: compaction.line_number,
        result_kind: COMPACT_INSPECT_RESULT_KIND,
        remote_response_note: COMPACT_INSPECT_REMOTE_NOTE,
        message: &compaction.compacted.message,
        replacement_history_item_count: compaction
            .compacted
            .replacement_history
            .as_ref()
            .map_or(0, Vec::len),
        replacement_history: &compaction.compacted.replacement_history,
    }
}

fn preview_json(items: &[ResponseItem]) -> String {
    let json = serde_json::to_string_pretty(items)
        .unwrap_or_else(|err| format!("Failed to serialize replacement history preview: {err}"));
    truncate_chars(&json, PREVIEW_CHAR_LIMIT)
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated = text.chars().take(limit).collect::<String>();
    truncated.push_str("\n... truncated; see the full JSON file above ...");
    truncated
}

fn report_file_stem(rollout_path: &Path, thread_id: Option<&ThreadId>) -> String {
    let raw = thread_id
        .map(ToString::to_string)
        .or_else(|| {
            rollout_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "rollout".to_string());
    sanitize_file_stem(&raw)
}

fn sanitize_file_stem(raw: &str) -> String {
    let sanitized = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "rollout".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
#[path = "compact_inspect_tests.rs"]
mod tests;
