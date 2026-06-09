use codex_protocol::models::ContentItem;
use codex_protocol::models::MessagePhase;
use codex_protocol::models::ResponseItem;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;

const MAX_PROGRESS_ITEMS: usize = 3;
const MAX_PROGRESS_ITEM_BYTES: usize = 512;
const MAX_CONTINUATION_SUPPLEMENT_BYTES: usize = 4 * 1024;

pub(crate) fn build_mid_turn_continuation_supplement(
    history_items: &[ResponseItem],
) -> Option<String> {
    let mut progress_items: Vec<String> = history_items
        .iter()
        .rev()
        .filter_map(textual_progress_from_response_item)
        .take(MAX_PROGRESS_ITEMS)
        .collect();
    progress_items.reverse();

    if progress_items.is_empty() {
        return None;
    }

    let mut supplement = String::from("你之前执行到这里：\n");
    for item in progress_items {
        supplement.push_str("- ");
        supplement.push_str(&item);
        supplement.push('\n');
    }
    supplement.push_str("\n如果任务尚未完成，应从这里继续，不要因为压缩而中断或丢弃后续工作。");

    Some(truncate_text(
        &supplement,
        TruncationPolicy::Bytes(MAX_CONTINUATION_SUPPLEMENT_BYTES),
    ))
}

fn textual_progress_from_response_item(item: &ResponseItem) -> Option<String> {
    let ResponseItem::Message {
        role,
        content,
        phase,
        ..
    } = item
    else {
        return None;
    };

    if role != "assistant" || matches!(phase, Some(MessagePhase::FinalAnswer)) {
        return None;
    }

    let text = content
        .iter()
        .filter_map(|item| match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                Some(text.as_str())
            }
            ContentItem::InputImage { .. } => None,
        })
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    Some(truncate_text(
        text,
        TruncationPolicy::Bytes(MAX_PROGRESS_ITEM_BYTES),
    ))
}

#[cfg(test)]
#[path = "post_compact_continuation_tests.rs"]
mod tests;
