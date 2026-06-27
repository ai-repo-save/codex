use crate::context::ContextRewindCarryForward;
use crate::context::ContextualUserFragment;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::ContextRewoundToAnchorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;
use codex_utils_output_truncation::approx_token_count;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ListedContextAnchor {
    pub(crate) anchor_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) label: Option<String>,
    pub(crate) created_at: i64,
    pub(crate) history_boundary: u64,
    pub(crate) response_items_since_anchor: u64,
    pub(crate) user_turns_since_anchor: u32,
    pub(crate) approx_tokens_since_anchor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ListContextAnchorsResponse {
    pub(crate) anchors: Vec<ListedContextAnchor>,
    pub(crate) current_history_items: u64,
    pub(crate) active_anchor_count: usize,
    pub(crate) invalidated_anchor_count: usize,
}

#[derive(Clone, Debug)]
struct ActiveAnchor {
    event: ContextAnchorSavedEvent,
    user_turn_total_at_save: u32,
}

fn count_user_turns_since_anchor(
    rollout_items: &[RolloutItem],
    anchor_id: &str,
) -> CodexResult<u32> {
    let mut anchor_index = None;
    for (index, item) in rollout_items.iter().enumerate() {
        match item {
            RolloutItem::Compacted(_) => {
                anchor_index = None;
            }
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event))
                if event.anchor_id == anchor_id =>
            {
                anchor_index = Some(index);
            }
            _ => {}
        }
    }

    let Some(anchor_index) = anchor_index else {
        return Err(CodexErr::InvalidRequest(format!(
            "unknown context anchor `{anchor_id}`"
        )));
    };

    let turn_count = rollout_items[anchor_index + 1..]
        .iter()
        .filter(|item| matches!(item, RolloutItem::EventMsg(EventMsg::UserMessage(_))))
        .count();
    Ok(u32::try_from(turn_count).unwrap_or(u32::MAX))
}

fn list_context_anchors_from_rollout(
    rollout_items: &[RolloutItem],
    current_history: &[ResponseItem],
    limit: usize,
) -> ListContextAnchorsResponse {
    let mut active_anchors: Vec<ActiveAnchor> = Vec::new();
    let mut invalidated_anchor_count = 0usize;
    let mut user_turn_total = 0u32;

    for item in rollout_items {
        match item {
            RolloutItem::Compacted(_) => {
                invalidated_anchor_count =
                    invalidated_anchor_count.saturating_add(active_anchors.len());
                active_anchors.clear();
                user_turn_total = 0;
            }
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event)) => {
                if let Some(existing_index) = active_anchors
                    .iter()
                    .position(|anchor| anchor.event.anchor_id == event.anchor_id)
                {
                    active_anchors.remove(existing_index);
                }
                active_anchors.push(ActiveAnchor {
                    event: event.clone(),
                    user_turn_total_at_save: user_turn_total,
                });
            }
            RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(rewind)) => {
                if let Some(anchor_index) = active_anchors
                    .iter()
                    .position(|anchor| anchor.event.anchor_id == rewind.anchor_id)
                {
                    let removed_count = active_anchors.len().saturating_sub(anchor_index + 1);
                    invalidated_anchor_count =
                        invalidated_anchor_count.saturating_add(removed_count);
                    active_anchors.truncate(anchor_index + 1);
                    user_turn_total = active_anchors[anchor_index].user_turn_total_at_save;
                }
            }
            RolloutItem::EventMsg(EventMsg::UserMessage(_)) => {
                user_turn_total = user_turn_total.saturating_add(1);
            }
            RolloutItem::EventMsg(EventMsg::ThreadRolledBack(rollback)) => {
                user_turn_total = user_turn_total.saturating_sub(rollback.num_turns);
            }
            _ => {}
        }
    }

    let current_history_items = u64::try_from(current_history.len()).unwrap_or(u64::MAX);
    let active_anchor_count = active_anchors.len();
    let anchors = active_anchors
        .iter()
        .rev()
        .take(limit)
        .map(|anchor| {
            let history_boundary = anchor.event.history_boundary;
            let response_items_since_anchor =
                current_history_items.saturating_sub(history_boundary);
            let approx_tokens_since_anchor = usize::try_from(history_boundary)
                .ok()
                .and_then(|boundary| current_history.get(boundary..))
                .map(approx_tokens_for_items)
                .unwrap_or_default();
            ListedContextAnchor {
                anchor_id: anchor.event.anchor_id.clone(),
                label: anchor.event.label.clone(),
                created_at: anchor.event.created_at,
                history_boundary,
                response_items_since_anchor,
                user_turns_since_anchor: user_turn_total
                    .saturating_sub(anchor.user_turn_total_at_save),
                approx_tokens_since_anchor,
            }
        })
        .collect();

    ListContextAnchorsResponse {
        anchors,
        current_history_items,
        active_anchor_count,
        invalidated_anchor_count,
    }
}

fn approx_tokens_for_items(items: &[ResponseItem]) -> usize {
    items
        .iter()
        .map(|item| {
            serde_json::to_string(item)
                .ok()
                .map(|text| approx_token_count(&text))
                .unwrap_or_default()
        })
        .sum()
}

impl Session {
    pub(crate) async fn save_context_anchor(
        &self,
        anchor_id: String,
        label: Option<String>,
        created_at: i64,
    ) -> CodexResult<ContextAnchorSavedEvent> {
        self.live_thread_for_persistence("save context anchor")
            .map_err(|err| CodexErr::InvalidRequest(err.to_string()))?;

        let history_boundary =
            u64::try_from(self.clone_history().await.raw_items().len()).unwrap_or(u64::MAX);
        let event = ContextAnchorSavedEvent {
            anchor_id,
            label,
            history_boundary,
            created_at,
        };
        self.persist_rollout_items(&[RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(
            event.clone(),
        ))])
        .await;
        self.flush_rollout().await.map_err(CodexErr::Io)?;
        Ok(event)
    }

    pub(crate) async fn rewind_context_to_anchor(
        &self,
        turn_context: &TurnContext,
        anchor_id: String,
        note: String,
    ) -> CodexResult<ContextRewoundToAnchorEvent> {
        let live_thread = self
            .live_thread_for_persistence("rewind context to anchor")
            .map_err(|err| CodexErr::InvalidRequest(err.to_string()))?;
        live_thread.flush().await.map_err(|err| {
            CodexErr::Io(std::io::Error::other(format!(
                "failed to flush thread persistence for context rewind replay: {err}"
            )))
        })?;

        let stored_history = live_thread
            .load_history(/*include_archived*/ false)
            .await
            .map_err(|err| {
                CodexErr::Io(std::io::Error::other(format!(
                    "failed to load thread history for context rewind replay: {err}"
                )))
            })?;
        let dropped_turns = count_user_turns_since_anchor(&stored_history.items, &anchor_id)?;
        let rewind_event = ContextRewoundToAnchorEvent {
            anchor_id,
            dropped_turns,
            note,
        };
        let replay_items = stored_history
            .items
            .iter()
            .cloned()
            .chain(std::iter::once(RolloutItem::EventMsg(
                EventMsg::ContextRewoundToAnchor(rewind_event.clone()),
            )))
            .collect::<Vec<_>>();
        self.apply_rollout_reconstruction(turn_context, replay_items.as_slice())
            .await;
        self.recompute_token_usage(turn_context).await;

        self.persist_rollout_items(&[RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(
            rewind_event.clone(),
        ))])
        .await;
        self.flush_rollout().await.map_err(CodexErr::Io)?;
        Ok(rewind_event)
    }

    pub(crate) async fn list_context_anchors(
        &self,
        limit: usize,
    ) -> CodexResult<ListContextAnchorsResponse> {
        let live_thread = self
            .live_thread_for_persistence("list context anchors")
            .map_err(|err| CodexErr::InvalidRequest(err.to_string()))?;
        live_thread.flush().await.map_err(|err| {
            CodexErr::Io(std::io::Error::other(format!(
                "failed to flush thread persistence for context anchor listing: {err}"
            )))
        })?;

        let stored_history = live_thread
            .load_history(/*include_archived*/ false)
            .await
            .map_err(|err| {
                CodexErr::Io(std::io::Error::other(format!(
                    "failed to load thread history for context anchor listing: {err}"
                )))
            })?;
        let current_history = self.clone_history().await;
        Ok(list_context_anchors_from_rollout(
            &stored_history.items,
            current_history.raw_items(),
            limit,
        ))
    }
}

pub(super) fn context_rewind_carry_forward_item(
    anchor_id: impl Into<String>,
    note: impl Into<String>,
) -> ResponseItem {
    ContextualUserFragment::into(ContextRewindCarryForward::new(anchor_id, note))
}

#[cfg(test)]
#[path = "context_anchor_tests.rs"]
mod tests;
