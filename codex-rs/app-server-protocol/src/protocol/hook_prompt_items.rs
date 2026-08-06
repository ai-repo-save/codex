//! Fork-owned mapping helpers for HookPrompt thread items.
//!
//! Center history dispatchers call these helpers so response-item parsing for
//! prompt hooks stays out of the shared history reducer.

use crate::protocol::v2::HookPromptFragment;
use crate::protocol::v2::ThreadItem;
use codex_protocol::items::parse_hook_prompt_message;
use codex_protocol::models::ResponseItem;

pub(crate) fn hook_prompt_item_from_response_item(item: &ResponseItem) -> Option<ThreadItem> {
    let ResponseItem::Message {
        role, content, id, ..
    } = item
    else {
        return None;
    };

    if role != "user" {
        return None;
    }

    let hook_prompt = parse_hook_prompt_message(id.as_deref(), content)?;
    Some(ThreadItem::HookPrompt {
        id: hook_prompt.id,
        fragments: hook_prompt
            .fragments
            .into_iter()
            .map(HookPromptFragment::from)
            .collect(),
    })
}
