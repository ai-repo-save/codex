use crate::context::ContextRewindCarryForward;
use crate::context::ContextualUserFragment;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_extension_api::RewindContextContributionInput;
use codex_protocol::config_types::ModeKind;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::ContextAnchorSavedEvent;
use codex_protocol::protocol::ContextRewoundToAnchorEvent;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RolloutItem;
use codex_utils_output_truncation::approx_token_count;
use serde::Serialize;
use uuid::Uuid;

mod contribution;
mod plan;

use contribution::RewindContributions;
use plan::RewindPlan;

pub(crate) const CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT: u32 = 20;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) collaboration_mode_kind: Option<ModeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) compatible_with_current_mode: Option<bool>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct ContextRewindBenefit {
    response_items_reclaimed: u64,
    approx_tokens_reclaimed: u64,
    reclaim_threshold_percent: u32,
    reclaim_threshold_tokens: Option<u64>,
    reclaim_threshold_met: Option<bool>,
}

#[derive(Clone, Debug)]
pub(crate) enum RewindContextToAnchorResult {
    Rewound {
        rewind_event: ContextRewoundToAnchorEvent,
        replacement_anchor: ContextAnchorSavedEvent,
    },
    Rejected(ContextRewindRejected),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContextRewindRejected {
    UnknownAnchor {
        anchor_id: String,
        replacement_anchor_id: Option<String>,
    },
    IncompatibleCollaborationMode {
        anchor_id: String,
        anchor_collaboration_mode: ModeKind,
        current_collaboration_mode: ModeKind,
    },
    BelowThreshold(ContextRewindThresholdRejected),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ContextRewindThresholdRejected {
    pub(crate) anchor_id: String,
    pub(crate) dropped_turns: u32,
    pub(crate) response_items_reclaimed: u64,
    pub(crate) approx_tokens_reclaimed: u64,
    pub(crate) reclaim_threshold_percent: u32,
    pub(crate) reclaim_threshold_tokens: Option<u64>,
    pub(crate) reclaim_threshold_met: Option<bool>,
    pub(crate) reason: ContextRewindRejectionReason,
    pub(crate) min_reclaim_percent: i64,
    pub(crate) min_reclaim_threshold_tokens: Option<u64>,
    pub(crate) model_context_window: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ContextRewindRejectionReason {
    BelowMinReclaimPercent,
    UnknownContextWindowForMinReclaimPercent,
}

fn latest_active_anchor_event(
    rollout_items: &[RolloutItem],
    anchor_id: &str,
) -> Option<ContextAnchorSavedEvent> {
    let mut active_anchors: Vec<ContextAnchorSavedEvent> = Vec::new();
    let mut current_collaboration_mode_kind = None;
    for (index, item) in rollout_items.iter().enumerate() {
        match item {
            RolloutItem::Compacted(_) => {
                active_anchors.clear();
            }
            RolloutItem::TurnContext(turn_context) => {
                current_collaboration_mode_kind = turn_context
                    .collaboration_mode
                    .as_ref()
                    .map(|mode| mode.mode);
            }
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event)) => {
                if let Some(existing_index) = active_anchors
                    .iter()
                    .position(|anchor| anchor.anchor_id == event.anchor_id)
                {
                    active_anchors.remove(existing_index);
                }
                let mut event = event.clone();
                if event.collaboration_mode_kind.is_none() {
                    event.collaboration_mode_kind = current_collaboration_mode_kind;
                }
                active_anchors.push(event);
            }
            RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(rewind)) => {
                if context_rewind_is_committed(rollout_items, index, rewind)
                    && active_anchors
                        .iter()
                        .position(|anchor| anchor.anchor_id == rewind.anchor_id)
                        .is_some()
                {
                    active_anchors.clear();
                }
            }
            _ => {}
        }
    }

    active_anchors
        .into_iter()
        .find(|anchor| anchor.anchor_id == anchor_id)
}

fn active_replacement_anchor_id(
    rollout_items: &[RolloutItem],
    invalidated_anchor_id: &str,
) -> Option<String> {
    let segment_start = rollout_items
        .iter()
        .rposition(|item| matches!(item, RolloutItem::Compacted(_)))
        .map_or(0, |index| index.saturating_add(1));
    let mut active_anchor_ids = Vec::new();
    let mut replacements = Vec::new();

    for (offset, item) in rollout_items[segment_start..].iter().enumerate() {
        match item {
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event)) => {
                if !active_anchor_ids.contains(&event.anchor_id) {
                    active_anchor_ids.push(event.anchor_id.clone());
                }
            }
            RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(rewind)) => {
                if context_rewind_is_committed(
                    rollout_items,
                    segment_start.saturating_add(offset),
                    rewind,
                ) && active_anchor_ids.contains(&rewind.anchor_id)
                {
                    active_anchor_ids.clear();
                    if let Some(replacement_anchor_id) = &rewind.replacement_anchor_id {
                        replacements
                            .push((rewind.anchor_id.clone(), replacement_anchor_id.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    let mut candidate = invalidated_anchor_id;
    for _ in 0..replacements.len() {
        let Some((_, replacement_anchor_id)) = replacements
            .iter()
            .rev()
            .find(|(anchor_id, _)| anchor_id == candidate)
        else {
            break;
        };
        candidate = replacement_anchor_id;
    }

    active_anchor_ids
        .iter()
        .find(|anchor_id| anchor_id.as_str() == candidate && candidate != invalidated_anchor_id)
        .cloned()
}

fn count_user_turns_since_anchor(
    rollout_items: &[RolloutItem],
    anchor_id: &str,
) -> CodexResult<u32> {
    let mut active_anchors: Vec<(String, u32)> = Vec::new();
    let mut user_turn_total = 0u32;
    for (index, item) in rollout_items.iter().enumerate() {
        match item {
            RolloutItem::Compacted(_) => {
                active_anchors.clear();
                user_turn_total = 0;
            }
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event)) => {
                if let Some(existing_index) = active_anchors
                    .iter()
                    .position(|(active_anchor_id, _)| active_anchor_id == &event.anchor_id)
                {
                    active_anchors.remove(existing_index);
                }
                active_anchors.push((event.anchor_id.clone(), user_turn_total));
            }
            RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(rewind)) => {
                if context_rewind_is_committed(rollout_items, index, rewind)
                    && let Some(anchor_index) = active_anchors
                        .iter()
                        .position(|(active_anchor_id, _)| active_anchor_id == &rewind.anchor_id)
                {
                    user_turn_total = active_anchors[anchor_index].1;
                    active_anchors.clear();
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

    active_anchors
        .into_iter()
        .find(|(active_anchor_id, _)| active_anchor_id == anchor_id)
        .map(|(_, user_turn_total_at_save)| user_turn_total.saturating_sub(user_turn_total_at_save))
        .ok_or_else(|| CodexErr::InvalidRequest(format!("unknown context anchor `{anchor_id}`")))
}

fn completed_turn_items_since_anchor(
    rollout_items: &[RolloutItem],
    anchor_id: &str,
) -> Vec<TurnItem> {
    let Some(anchor_index) = rollout_items.iter().rposition(|item| {
        matches!(
            item,
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event))
                if event.anchor_id == anchor_id
        )
    }) else {
        return Vec::new();
    };

    rollout_items[anchor_index.saturating_add(1)..]
        .iter()
        .filter_map(|item| match item {
            RolloutItem::EventMsg(EventMsg::ItemCompleted(event)) => Some(event.item.clone()),
            _ => None,
        })
        .collect()
}

fn list_context_anchors_from_rollout(
    rollout_items: &[RolloutItem],
    current_history: &[ResponseItem],
    limit: usize,
    current_collaboration_mode_kind: ModeKind,
) -> ListContextAnchorsResponse {
    let mut active_anchors: Vec<ActiveAnchor> = Vec::new();
    let mut invalidated_anchor_count = 0usize;
    let mut user_turn_total = 0u32;
    let mut latest_collaboration_mode_kind = None;

    for (index, item) in rollout_items.iter().enumerate() {
        match item {
            RolloutItem::Compacted(_) => {
                invalidated_anchor_count =
                    invalidated_anchor_count.saturating_add(active_anchors.len());
                active_anchors.clear();
                user_turn_total = 0;
            }
            RolloutItem::TurnContext(turn_context) => {
                latest_collaboration_mode_kind = turn_context
                    .collaboration_mode
                    .as_ref()
                    .map(|mode| mode.mode);
            }
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(event)) => {
                if let Some(existing_index) = active_anchors
                    .iter()
                    .position(|anchor| anchor.event.anchor_id == event.anchor_id)
                {
                    active_anchors.remove(existing_index);
                }
                let mut event = event.clone();
                if event.collaboration_mode_kind.is_none() {
                    event.collaboration_mode_kind = latest_collaboration_mode_kind;
                }
                active_anchors.push(ActiveAnchor {
                    event,
                    user_turn_total_at_save: user_turn_total,
                });
            }
            RolloutItem::EventMsg(EventMsg::ContextRewoundToAnchor(rewind)) => {
                if context_rewind_is_committed(rollout_items, index, rewind)
                    && let Some(anchor_index) = active_anchors
                        .iter()
                        .position(|anchor| anchor.event.anchor_id == rewind.anchor_id)
                {
                    let user_turn_total_at_save =
                        active_anchors[anchor_index].user_turn_total_at_save;
                    let removed_count = active_anchors.len();
                    invalidated_anchor_count =
                        invalidated_anchor_count.saturating_add(removed_count);
                    active_anchors.clear();
                    user_turn_total = user_turn_total_at_save;
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
            let compatible_with_current_mode = anchor
                .event
                .collaboration_mode_kind
                .map(|anchor_mode| anchor_mode == current_collaboration_mode_kind);
            ListedContextAnchor {
                anchor_id: anchor.event.anchor_id.clone(),
                label: anchor.event.label.clone(),
                created_at: anchor.event.created_at,
                history_boundary,
                response_items_since_anchor,
                user_turns_since_anchor: user_turn_total
                    .saturating_sub(anchor.user_turn_total_at_save),
                approx_tokens_since_anchor,
                collaboration_mode_kind: anchor.event.collaboration_mode_kind,
                compatible_with_current_mode,
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

fn context_rewind_is_committed(
    rollout_items: &[RolloutItem],
    rewind_index: usize,
    rewind: &ContextRewoundToAnchorEvent,
) -> bool {
    let Some(replacement_anchor_id) = rewind.replacement_anchor_id.as_deref() else {
        return true;
    };
    for item in &rollout_items[rewind_index.saturating_add(1)..] {
        match item {
            RolloutItem::ResponseItem(_) => {}
            RolloutItem::EventMsg(EventMsg::ContextAnchorSaved(anchor)) => {
                return anchor.anchor_id == replacement_anchor_id;
            }
            _ => return false,
        }
    }
    false
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

fn rewind_benefit_since_anchor(
    anchor: &ContextAnchorSavedEvent,
    current_history: &[ResponseItem],
    current_rewind_call_id: &str,
    model_context_window: Option<i64>,
) -> ContextRewindBenefit {
    let reclaimed_items = usize::try_from(anchor.history_boundary)
        .ok()
        .and_then(|boundary| current_history.get(boundary..))
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    !matches!(
                        item,
                        ResponseItem::FunctionCall { call_id, .. }
                            if call_id == current_rewind_call_id
                    )
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let approx_tokens_reclaimed =
        u64::try_from(approx_tokens_for_items(&reclaimed_items)).unwrap_or(u64::MAX);
    let reclaim_threshold_tokens = model_context_window
        .and_then(|context_window| u64::try_from(context_window).ok())
        .map(|context_window| {
            context_window.saturating_mul(u64::from(CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT))
                / 100
        });

    ContextRewindBenefit {
        response_items_reclaimed: u64::try_from(reclaimed_items.len()).unwrap_or(u64::MAX),
        approx_tokens_reclaimed,
        reclaim_threshold_percent: CONTEXT_REWIND_SIGNIFICANT_RECLAIM_PERCENT,
        reclaim_threshold_tokens,
        reclaim_threshold_met: reclaim_threshold_tokens
            .map(|threshold| approx_tokens_reclaimed >= threshold),
    }
}

fn evaluate_min_reclaim_percent(
    benefit: &ContextRewindBenefit,
    model_context_window: Option<i64>,
    min_reclaim_percent: i64,
) -> CodexResult<Option<(ContextRewindRejectionReason, Option<u64>, Option<u64>)>> {
    if min_reclaim_percent == 0 {
        return Ok(None);
    }

    let min_reclaim_percent_u64 = u64::try_from(min_reclaim_percent).map_err(|_| {
        CodexErr::InvalidRequest(format!(
            "context_rewind.min_reclaim_percent must be at least 0, got {min_reclaim_percent}"
        ))
    })?;
    let Some(context_window) = model_context_window.and_then(|value| u64::try_from(value).ok())
    else {
        return Ok(Some((
            ContextRewindRejectionReason::UnknownContextWindowForMinReclaimPercent,
            None,
            None,
        )));
    };
    let threshold_tokens = context_window.saturating_mul(min_reclaim_percent_u64) / 100;
    if benefit.approx_tokens_reclaimed < threshold_tokens {
        return Ok(Some((
            ContextRewindRejectionReason::BelowMinReclaimPercent,
            Some(threshold_tokens),
            Some(context_window),
        )));
    }

    Ok(None)
}

impl Session {
    pub(crate) async fn save_context_anchor(
        &self,
        turn_context: &TurnContext,
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
            collaboration_mode_kind: Some(turn_context.collaboration_mode().mode),
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
        current_rewind_call_id: &str,
        note: String,
    ) -> CodexResult<RewindContextToAnchorResult> {
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
        let Some(active_anchor) = latest_active_anchor_event(&stored_history.items, &anchor_id)
        else {
            return Ok(RewindContextToAnchorResult::Rejected(
                ContextRewindRejected::UnknownAnchor {
                    replacement_anchor_id: active_replacement_anchor_id(
                        &stored_history.items,
                        &anchor_id,
                    ),
                    anchor_id,
                },
            ));
        };
        if let Some(anchor_collaboration_mode) = active_anchor.collaboration_mode_kind
            && anchor_collaboration_mode != turn_context.collaboration_mode().mode
        {
            return Ok(RewindContextToAnchorResult::Rejected(
                ContextRewindRejected::IncompatibleCollaborationMode {
                    anchor_id,
                    anchor_collaboration_mode,
                    current_collaboration_mode: turn_context.collaboration_mode().mode,
                },
            ));
        }
        let dropped_turns = count_user_turns_since_anchor(&stored_history.items, &anchor_id)?;
        let current_history = self.clone_history().await;
        let benefit = rewind_benefit_since_anchor(
            &active_anchor,
            current_history.raw_items(),
            current_rewind_call_id,
            turn_context.model_context_window(),
        );
        let min_reclaim_percent = turn_context.config.context_rewind.min_reclaim_percent;
        if let Some((reason, min_reclaim_threshold_tokens, model_context_window)) =
            evaluate_min_reclaim_percent(
                &benefit,
                turn_context.model_context_window(),
                min_reclaim_percent,
            )?
        {
            return Ok(RewindContextToAnchorResult::Rejected(
                ContextRewindRejected::BelowThreshold(ContextRewindThresholdRejected {
                    anchor_id,
                    dropped_turns,
                    response_items_reclaimed: benefit.response_items_reclaimed,
                    approx_tokens_reclaimed: benefit.approx_tokens_reclaimed,
                    reclaim_threshold_percent: benefit.reclaim_threshold_percent,
                    reclaim_threshold_tokens: benefit.reclaim_threshold_tokens,
                    reclaim_threshold_met: benefit.reclaim_threshold_met,
                    reason,
                    min_reclaim_percent,
                    min_reclaim_threshold_tokens,
                    model_context_window,
                }),
            ));
        }
        let replacement_anchor_id = format!("ctx-{}", Uuid::now_v7());
        let rewind_event = ContextRewoundToAnchorEvent {
            anchor_id: anchor_id.clone(),
            replacement_anchor_id: Some(replacement_anchor_id.clone()),
            dropped_turns,
            response_items_reclaimed: benefit.response_items_reclaimed,
            approx_tokens_reclaimed: benefit.approx_tokens_reclaimed,
            reclaim_threshold_percent: benefit.reclaim_threshold_percent,
            reclaim_threshold_tokens: benefit.reclaim_threshold_tokens,
            reclaim_threshold_met: benefit.reclaim_threshold_met,
            note,
        };
        let completed_items =
            completed_turn_items_since_anchor(&stored_history.items, &active_anchor.anchor_id);
        let mut prompt_fragments = Vec::new();
        let mut contextual_fragments = Vec::new();
        for contributor in self.services.extensions.context_contributors() {
            let input = RewindContextContributionInput {
                session_store: &self.services.session_extension_data,
                thread_store: &self.services.thread_extension_data,
                completed_items: &completed_items,
            };
            prompt_fragments.extend(contributor.contribute_rewind_context(input).await);
            contextual_fragments
                .extend(contributor.contribute_rewind_context_fragments(input).await);
        }
        let contribution_items =
            RewindContributions::from_fragments(prompt_fragments, contextual_fragments)
                .into_response_items();
        let plan = RewindPlan::new(
            stored_history.items,
            rewind_event,
            contribution_items,
            replacement_anchor_id,
        );
        let created_at = crate::turn_timing::now_unix_timestamp_ms() / 1000;
        let collaboration_mode_kind = turn_context.collaboration_mode().mode;
        let provisional_plan = plan.clone().finalize(
            /*history_boundary*/ 0,
            created_at,
            collaboration_mode_kind,
        );
        let reconstructed = self
            .reconstruct_history_from_rollout(turn_context, &provisional_plan.replay_items)
            .await;
        let history_boundary = u64::try_from(reconstructed.history.len()).unwrap_or(u64::MAX);
        let plan = plan.finalize(history_boundary, created_at, collaboration_mode_kind);

        live_thread
            .append_items(&plan.persisted_items)
            .await
            .map_err(|err| {
                CodexErr::Io(std::io::Error::other(format!(
                    "failed to persist context rewind transaction: {err}"
                )))
            })?;
        live_thread.flush().await.map_err(|err| {
            CodexErr::Io(std::io::Error::other(format!(
                "failed to flush context rewind transaction: {err}"
            )))
        })?;

        self.apply_rollout_reconstruction(turn_context, &plan.replay_items)
            .await;
        self.recompute_token_usage(turn_context).await;
        Ok(RewindContextToAnchorResult::Rewound {
            rewind_event: plan.rewind_event,
            replacement_anchor: plan.replacement_anchor,
        })
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
            self.collaboration_mode().await.mode,
        ))
    }
}

pub(super) fn context_rewind_carry_forward_item(
    anchor_id: impl Into<String>,
    replacement_anchor_id: Option<String>,
    dropped_turns: u32,
    response_items_reclaimed: u64,
    approx_tokens_reclaimed: u64,
    reclaim_threshold_percent: u32,
    reclaim_threshold_tokens: Option<u64>,
    reclaim_threshold_met: Option<bool>,
    note: impl Into<String>,
) -> ResponseItem {
    ContextualUserFragment::into(ContextRewindCarryForward::new(
        anchor_id,
        replacement_anchor_id,
        dropped_turns,
        response_items_reclaimed,
        approx_tokens_reclaimed,
        reclaim_threshold_percent,
        reclaim_threshold_tokens,
        reclaim_threshold_met,
        note,
    ))
}

#[cfg(test)]
#[path = "context_anchor_tests.rs"]
mod tests;
