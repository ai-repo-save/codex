//! Mid-turn context lifecycle tool side effects (anchors / rewind).
//!
//! `session/turn.rs` drains in-flight tool futures; fork-owned context-anchor
//! application lives here so the upstream turn loop only calls a thin entrypoint.

use std::sync::Arc;

use codex_protocol::models::FunctionCallOutputPayload;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use tracing::instrument;

use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use crate::session::context_anchor::ContextRewindRejectionReason as SessionContextRewindRejectionReason;
use crate::session::context_anchor::RewindContextToAnchorResult;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use crate::stream_events_utils::InFlightToolOutput;
use crate::stream_events_utils::mark_thread_memory_mode_polluted_if_external_context;
use crate::stream_events_utils::parse_function_call_output;
use crate::tools::handlers::context_anchor::ListContextAnchorsRequest;
use crate::tools::handlers::context_anchor::RewindContextToAnchorRejectionReason;
use crate::tools::handlers::context_anchor::RewindContextToAnchorRequest;
use crate::tools::handlers::context_anchor::RewindContextToAnchorResponse;
use crate::tools::handlers::context_anchor::SaveContextAnchorResponse;
use crate::tools::handlers::context_anchor_spec::LIST_CONTEXT_ANCHORS_TOOL_NAME;
use crate::tools::handlers::context_anchor_spec::REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME;
use crate::tools::handlers::context_anchor_spec::SAVE_CONTEXT_ANCHOR_TOOL_NAME;

async fn retained_response_items_for_context_rewind(
    sess: &Session,
    call_id: &str,
) -> CodexResult<Vec<ResponseItem>> {
    let history = sess.clone_history().await;
    let Some(function_call) = history.raw_items().iter().rev().find(|item| {
        matches!(
            item,
            ResponseItem::FunctionCall {
                call_id: existing_call_id,
                name,
                namespace,
                ..
            } if existing_call_id == call_id
                && name == REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME
                && namespace.is_none()
        )
    }) else {
        return Err(CodexErr::Fatal(format!(
            "missing {REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME} function call for call id: {call_id}"
        )));
    };

    Ok(vec![function_call.clone()])
}

/// Returns true when `output` is a context-anchor lifecycle variant handled here.
#[instrument(level = "trace", skip_all)]
pub(crate) async fn apply_context_lifecycle_tool_output(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    output: InFlightToolOutput,
) -> CodexResult<bool> {
    match output {
        InFlightToolOutput::SaveContextAnchor(response_input) => {
            let (_, response) = parse_function_call_output::<SaveContextAnchorResponse>(
                &response_input,
                SAVE_CONTEXT_ANCHOR_TOOL_NAME,
            )?;
            let response_item = response_input.into();
            sess.record_conversation_items(turn_context, std::slice::from_ref(&response_item))
                .await;
            mark_thread_memory_mode_polluted_if_external_context(
                sess.as_ref(),
                turn_context.as_ref(),
                &response_item,
            )
            .await;
            let event = sess
                .save_context_anchor(
                    turn_context,
                    response.anchor_id,
                    response.label,
                    response.created_at,
                )
                .await?;
            sess.deliver_persisted_event(turn_context, EventMsg::ContextAnchorSaved(event))
                .await;
            Ok(true)
        }
        InFlightToolOutput::ListContextAnchors(response_input) => {
            let (call_id, request) = parse_function_call_output::<ListContextAnchorsRequest>(
                &response_input,
                LIST_CONTEXT_ANCHORS_TOOL_NAME,
            )?;
            let response = sess.list_context_anchors(request.limit).await?;
            let text = serde_json::to_string(&response).map_err(|err| {
                CodexErr::Fatal(format!(
                    "failed to serialize {LIST_CONTEXT_ANCHORS_TOOL_NAME} response: {err}"
                ))
            })?;
            let mut output = FunctionCallOutputPayload::from_text(text);
            output.success = Some(true);
            let response_item: ResponseItem =
                ResponseInputItem::FunctionCallOutput { call_id, output }.into();
            sess.record_conversation_items(turn_context, std::slice::from_ref(&response_item))
                .await;
            Ok(true)
        }
        InFlightToolOutput::RewindContextToAnchor(response_input) => {
            apply_rewind_context_to_anchor(sess, turn_context, response_input).await?;
            Ok(true)
        }
        InFlightToolOutput::Response(_) | InFlightToolOutput::RequestContextCompaction(_) => {
            Ok(false)
        }
    }
}

async fn apply_rewind_context_to_anchor(
    sess: &Arc<Session>,
    turn_context: &Arc<TurnContext>,
    response_input: ResponseInputItem,
) -> CodexResult<()> {
    let (call_id, request) = parse_function_call_output::<RewindContextToAnchorRequest>(
        &response_input,
        REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME,
    )?;
    let retained_response_items =
        retained_response_items_for_context_rewind(sess.as_ref(), &call_id).await?;
    let rewind_event = sess
        .rewind_context_to_anchor(
            turn_context.as_ref(),
            request.anchor_id,
            &call_id,
            request.note,
        )
        .await?;
    let (response, response_items) = match rewind_event {
        RewindContextToAnchorResult::Rewound {
            rewind_event,
            replacement_anchor,
        } => {
            sess.deliver_persisted_event(
                turn_context,
                EventMsg::ContextRewoundToAnchor(rewind_event.clone()),
            )
            .await;
            sess.deliver_persisted_event(
                turn_context,
                EventMsg::ContextAnchorSaved(replacement_anchor),
            )
            .await;
            let response = RewindContextToAnchorResponse::Rewound {
                anchor_id: rewind_event.anchor_id,
                replacement_anchor_id: rewind_event.replacement_anchor_id.expect(
                    "successful context rewind should create replacement anchor",
                ),
                dropped_turns: rewind_event.dropped_turns,
                response_items_reclaimed: rewind_event.response_items_reclaimed,
                approx_tokens_reclaimed: rewind_event.approx_tokens_reclaimed,
                reclaim_threshold_percent: rewind_event.reclaim_threshold_percent,
                reclaim_threshold_tokens: rewind_event.reclaim_threshold_tokens,
                reclaim_threshold_met: rewind_event.reclaim_threshold_met,
            };
            (response, retained_response_items)
        }
        RewindContextToAnchorResult::Rejected(rejection) => {
            let response = match rejection {
                crate::session::context_anchor::ContextRewindRejected::UnknownAnchor {
                    anchor_id,
                    replacement_anchor_id,
                } => RewindContextToAnchorResponse::Rejected {
                    anchor_id,
                    replacement_anchor_id,
                    dropped_turns: None,
                    response_items_reclaimed: None,
                    approx_tokens_reclaimed: None,
                    reclaim_threshold_percent: None,
                    reclaim_threshold_tokens: None,
                    reclaim_threshold_met: None,
                    reason: RewindContextToAnchorRejectionReason::UnknownContextAnchor,
                    min_reclaim_percent: None,
                    min_reclaim_threshold_tokens: None,
                    model_context_window: None,
                    anchor_collaboration_mode: None,
                    current_collaboration_mode: None,
                },
                crate::session::context_anchor::ContextRewindRejected::IncompatibleCollaborationMode {
                    anchor_id,
                    anchor_collaboration_mode,
                    current_collaboration_mode,
                } => RewindContextToAnchorResponse::Rejected {
                    anchor_id,
                    replacement_anchor_id: None,
                    dropped_turns: None,
                    response_items_reclaimed: None,
                    approx_tokens_reclaimed: None,
                    reclaim_threshold_percent: None,
                    reclaim_threshold_tokens: None,
                    reclaim_threshold_met: None,
                    reason: RewindContextToAnchorRejectionReason::IncompatibleCollaborationMode,
                    min_reclaim_percent: None,
                    min_reclaim_threshold_tokens: None,
                    model_context_window: None,
                    anchor_collaboration_mode: Some(anchor_collaboration_mode),
                    current_collaboration_mode: Some(current_collaboration_mode),
                },
                crate::session::context_anchor::ContextRewindRejected::BelowThreshold(rejection) => {
                    let reason = match rejection.reason {
                        SessionContextRewindRejectionReason::BelowMinReclaimPercent => {
                            RewindContextToAnchorRejectionReason::BelowMinReclaimPercent
                        }
                        SessionContextRewindRejectionReason::UnknownContextWindowForMinReclaimPercent => {
                            RewindContextToAnchorRejectionReason::UnknownContextWindowForMinReclaimPercent
                        }
                    };
                    RewindContextToAnchorResponse::Rejected {
                        anchor_id: rejection.anchor_id,
                        replacement_anchor_id: None,
                        dropped_turns: Some(rejection.dropped_turns),
                        response_items_reclaimed: Some(rejection.response_items_reclaimed),
                        approx_tokens_reclaimed: Some(rejection.approx_tokens_reclaimed),
                        reclaim_threshold_percent: Some(rejection.reclaim_threshold_percent),
                        reclaim_threshold_tokens: rejection.reclaim_threshold_tokens,
                        reclaim_threshold_met: rejection.reclaim_threshold_met,
                        reason,
                        min_reclaim_percent: Some(rejection.min_reclaim_percent),
                        min_reclaim_threshold_tokens: rejection.min_reclaim_threshold_tokens,
                        model_context_window: rejection.model_context_window,
                        anchor_collaboration_mode: None,
                        current_collaboration_mode: None,
                    }
                }
            };
            (response, Vec::new())
        }
    };
    let text = serde_json::to_string(&response).map_err(|err| {
        CodexErr::Fatal(format!(
            "failed to serialize {REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME} response: {err}"
        ))
    })?;
    let mut output = FunctionCallOutputPayload::from_text(text);
    output.success = Some(true);
    let response_item: ResponseItem =
        ResponseInputItem::FunctionCallOutput { call_id, output }.into();
    let retained_and_response = response_items
        .into_iter()
        .chain(std::iter::once(response_item))
        .collect::<Vec<_>>();
    sess.record_conversation_items(turn_context, &retained_and_response)
        .await;
    Ok(())
}

pub(crate) fn rewind_must_be_sole_tool_call(outputs: &[InFlightToolOutput]) -> CodexResult<()> {
    let context_rewind_count = outputs
        .iter()
        .filter(|output| matches!(output, InFlightToolOutput::RewindContextToAnchor(_)))
        .count();
    if context_rewind_count > 0 && outputs.len() > 1 {
        return Err(CodexErr::Fatal(format!(
            "{REWIND_CONTEXT_TO_ANCHOR_TOOL_NAME} must be the only tool call in a model response"
        )));
    }
    Ok(())
}
