//! Fork-owned mapping helpers for context-anchor history items.
//!
//! Center history/event dispatchers call these helpers and stay free of
//! anchor-specific field assembly and duplicate detection.

use crate::protocol::v2::ThreadItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::ContextRewoundToAnchorEvent;

pub(crate) fn context_anchor_saved_item(
    id: String,
    payload: &ContextAnchorSavedEvent,
) -> ThreadItem {
    ThreadItem::ContextAnchorSaved {
        id,
        anchor_id: payload.anchor_id.clone(),
        label: payload.label.clone(),
        history_boundary: payload.history_boundary,
        created_at: payload.created_at,
    }
}

pub(crate) fn context_anchor_rewound_item(
    id: String,
    payload: &ContextRewoundToAnchorEvent,
) -> ThreadItem {
    ThreadItem::ContextAnchorRewound {
        id,
        anchor_id: payload.anchor_id.clone(),
        dropped_turns: payload.dropped_turns,
        response_items_reclaimed: payload.response_items_reclaimed,
        approx_tokens_reclaimed: payload.approx_tokens_reclaimed,
        reclaim_threshold_percent: payload.reclaim_threshold_percent,
        reclaim_threshold_tokens: payload.reclaim_threshold_tokens,
        reclaim_threshold_met: payload.reclaim_threshold_met,
    }
}

pub(crate) fn is_duplicate_context_anchor_saved(
    last: Option<&ThreadItem>,
    payload: &ContextAnchorSavedEvent,
) -> bool {
    last.is_some_and(|last| {
        matches!(
            last,
            ThreadItem::ContextAnchorSaved {
                anchor_id,
                label,
                history_boundary,
                created_at,
                ..
            } if anchor_id == &payload.anchor_id
                && label == &payload.label
                && history_boundary == &payload.history_boundary
                && created_at == &payload.created_at
        )
    })
}

pub(crate) fn is_duplicate_context_anchor_rewound(
    last: Option<&ThreadItem>,
    payload: &ContextRewoundToAnchorEvent,
) -> bool {
    last.is_some_and(|last| {
        matches!(
            last,
            ThreadItem::ContextAnchorRewound {
                anchor_id,
                dropped_turns,
                response_items_reclaimed,
                approx_tokens_reclaimed,
                reclaim_threshold_percent,
                reclaim_threshold_tokens,
                reclaim_threshold_met,
                ..
            } if anchor_id == &payload.anchor_id
                && dropped_turns == &payload.dropped_turns
                && response_items_reclaimed == &payload.response_items_reclaimed
                && approx_tokens_reclaimed == &payload.approx_tokens_reclaimed
                && reclaim_threshold_percent == &payload.reclaim_threshold_percent
                && reclaim_threshold_tokens == &payload.reclaim_threshold_tokens
                && reclaim_threshold_met == &payload.reclaim_threshold_met
        )
    })
}
