//! Fork-owned mapping helpers for ImageGeneration thread items.

use crate::protocol::v2::ThreadItem;
use codex_extension_items::image_generation::ImageGenerationItem;
use codex_protocol::protocol::ImageGenerationBeginEvent;
use codex_protocol::protocol::ImageGenerationEndEvent;

pub(crate) fn image_generation_item_from_begin(payload: &ImageGenerationBeginEvent) -> ThreadItem {
    ThreadItem::ImageGeneration(ImageGenerationItem {
        id: payload.call_id.clone(),
        status: String::new(),
        revised_prompt: None,
        result: String::new(),
        saved_path: None,
    })
}

pub(crate) fn image_generation_item_from_end(payload: &ImageGenerationEndEvent) -> ThreadItem {
    ThreadItem::ImageGeneration(ImageGenerationItem {
        id: payload.call_id.clone(),
        status: payload.status.clone(),
        revised_prompt: payload.revised_prompt.clone(),
        result: payload.result.clone(),
        saved_path: payload.saved_path.clone(),
    })
}
