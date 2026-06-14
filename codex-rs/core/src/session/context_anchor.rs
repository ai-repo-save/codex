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

fn count_user_turns_since_anchor(
    rollout_items: &[RolloutItem],
    anchor_id: &str,
) -> CodexResult<u32> {
    let Some(anchor_index) = rollout_items.iter().rposition(|item| {
        matches!(
            item,
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event))
                if event.anchor_id == anchor_id
        )
    }) else {
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
        self.services.model_client.advance_window_generation();
        Ok(rewind_event)
    }
}

pub(super) fn context_rewind_carry_forward_item(
    anchor_id: impl Into<String>,
    note: impl Into<String>,
) -> ResponseItem {
    ContextualUserFragment::into(ContextRewindCarryForward::new(anchor_id, note))
}
