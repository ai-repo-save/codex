use codex_protocol::models::ContentItem;
use codex_protocol::models::ReasoningItemReasoningSummary;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::AgentStatus;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use serde::Serialize;
use std::collections::HashMap;

const DEFAULT_INSPECT_AGENT_TAIL_ITEMS: usize = 20;
const MAX_INSPECT_AGENT_TAIL_ITEMS: usize = 100;
const MAX_INSPECT_AGENT_ENTRY_BYTES: usize = 4096;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct InspectedAgent {
    pub(crate) agent_name: String,
    pub(crate) agent_status: AgentStatus,
    pub(crate) last_task_message: Option<String>,
    pub(crate) transcript_tail: Vec<TranscriptTailEntry>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct TranscriptTailEntry {
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) name: Option<String>,
    pub(crate) text: String,
}

fn inspect_tail_limit(tail_items: Option<usize>) -> usize {
    tail_items
        .unwrap_or(DEFAULT_INSPECT_AGENT_TAIL_ITEMS)
        .min(MAX_INSPECT_AGENT_TAIL_ITEMS)
}

fn content_items_to_plain_text(content: &[ContentItem]) -> Option<String> {
    let pieces = content
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text }
                if !text.trim().is_empty() =>
            {
                Some(text.as_str())
            }
            ContentItem::InputText { .. }
            | ContentItem::OutputText { .. }
            | ContentItem::InputImage { .. }
            | ContentItem::InputAudio { .. } => None,
        })
        .collect::<Vec<_>>();
    (!pieces.is_empty()).then(|| pieces.join("\n"))
}

fn truncate_transcript_entry_text(text: String) -> String {
    truncate_text(
        &text,
        TruncationPolicy::Bytes(MAX_INSPECT_AGENT_ENTRY_BYTES),
    )
}

fn transcript_tail_entry(
    role: impl Into<String>,
    kind: impl Into<String>,
    name: Option<String>,
    text: String,
) -> Option<TranscriptTailEntry> {
    (!text.trim().is_empty()).then(|| TranscriptTailEntry {
        role: role.into(),
        kind: kind.into(),
        name,
        text: truncate_transcript_entry_text(text),
    })
}

fn reasoning_summary_to_text(summary: &[ReasoningItemReasoningSummary]) -> Option<String> {
    let pieces = summary
        .iter()
        .map(|item| match item {
            ReasoningItemReasoningSummary::SummaryText { text } => text.as_str(),
        })
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>();
    (!pieces.is_empty()).then(|| pieces.join("\n"))
}

pub(crate) fn summarize_transcript_tail(
    items: &[ResponseItem],
    tail_items: Option<usize>,
) -> Vec<TranscriptTailEntry> {
    let mut entries = Vec::new();
    let mut tool_names_by_call_id = HashMap::new();

    for item in items {
        let entry = match item {
            ResponseItem::Message { role, content, .. } => {
                if role == "system" || role == "developer" {
                    None
                } else {
                    content_items_to_plain_text(content)
                        .and_then(|text| transcript_tail_entry(role.clone(), "message", None, text))
                }
            }
            ResponseItem::Reasoning { summary, .. } => {
                reasoning_summary_to_text(summary).and_then(|text| {
                    transcript_tail_entry("assistant", "reasoning_summary", None, text)
                })
            }
            ResponseItem::LocalShellCall { action, .. } => {
                serde_json::to_string(action).ok().and_then(|text| {
                    transcript_tail_entry(
                        "tool",
                        "local_shell_call",
                        Some("shell".to_string()),
                        text,
                    )
                })
            }
            ResponseItem::FunctionCall {
                call_id,
                name,
                namespace,
                arguments,
                ..
            } => {
                let tool_name = namespace
                    .as_ref()
                    .map(|namespace| format!("{namespace}.{name}"))
                    .unwrap_or_else(|| name.clone());
                tool_names_by_call_id.insert(call_id.clone(), tool_name.clone());
                transcript_tail_entry("tool", "function_call", Some(tool_name), arguments.clone())
            }
            ResponseItem::FunctionCallOutput {
                call_id, output, ..
            } => output.body.to_text().and_then(|text| {
                transcript_tail_entry(
                    "tool",
                    "function_call_output",
                    tool_names_by_call_id.get(call_id).cloned(),
                    text,
                )
            }),
            ResponseItem::CustomToolCall {
                call_id,
                name,
                input,
                ..
            } => {
                tool_names_by_call_id.insert(call_id.clone(), name.clone());
                transcript_tail_entry(
                    "tool",
                    "custom_tool_call",
                    Some(name.clone()),
                    input.clone(),
                )
            }
            ResponseItem::CustomToolCallOutput {
                call_id,
                name,
                output,
                ..
            } => output.body.to_text().and_then(|text| {
                transcript_tail_entry(
                    "tool",
                    "custom_tool_call_output",
                    name.clone()
                        .or_else(|| tool_names_by_call_id.get(call_id).cloned()),
                    text,
                )
            }),
            ResponseItem::ToolSearchCall {
                execution,
                arguments,
                ..
            } => serde_json::to_string(arguments).ok().and_then(|text| {
                transcript_tail_entry("tool", "tool_search_call", Some(execution.clone()), text)
            }),
            ResponseItem::ToolSearchOutput {
                execution, tools, ..
            } => serde_json::to_string(tools).ok().and_then(|text| {
                transcript_tail_entry("tool", "tool_search_output", Some(execution.clone()), text)
            }),
            ResponseItem::WebSearchCall { action, .. } => action
                .as_ref()
                .and_then(|action| serde_json::to_string(action).ok())
                .and_then(|text| {
                    transcript_tail_entry(
                        "tool",
                        "web_search_call",
                        Some("web_search".to_string()),
                        text,
                    )
                }),
            ResponseItem::ImageGenerationCall {
                revised_prompt,
                status,
                ..
            } => transcript_tail_entry(
                "tool",
                "image_generation_call",
                Some("image_generation".to_string()),
                revised_prompt.clone().unwrap_or_else(|| status.clone()),
            ),
            ResponseItem::AdditionalTools { .. } | ResponseItem::AgentMessage { .. } => None,
            ResponseItem::Compaction { .. }
            | ResponseItem::CompactionTrigger { .. }
            | ResponseItem::ContextCompaction { .. }
            | ResponseItem::Other => None,
        };

        if let Some(entry) = entry {
            entries.push(entry);
        }
    }

    let tail_limit = inspect_tail_limit(tail_items);
    let start = entries.len().saturating_sub(tail_limit);
    entries.into_iter().skip(start).collect()
}
