use crate::protocol::collaboration_items::CollaborationItemLifecycle;
use crate::protocol::collaboration_items::collaboration_item_from_event;
use crate::protocol::common::ServerNotification;
use crate::protocol::item_builders::build_command_execution_begin_item;
use crate::protocol::item_builders::build_command_execution_end_item;
use crate::protocol::item_builders::convert_patch_changes;
use crate::protocol::v2::AgentMessageDeltaNotification;
use crate::protocol::v2::CommandExecutionOutputDeltaNotification;
use crate::protocol::v2::DynamicToolCallOutputContentItem;
use crate::protocol::v2::DynamicToolCallStatus;
use crate::protocol::v2::FileChangePatchUpdatedNotification;
use crate::protocol::v2::ItemCompletedNotification;
use crate::protocol::v2::ItemStartedNotification;
use crate::protocol::v2::PlanDeltaNotification;
use crate::protocol::v2::ReasoningSummaryPartAddedNotification;
use crate::protocol::v2::ReasoningSummaryTextDeltaNotification;
use crate::protocol::v2::ReasoningTextDeltaNotification;
use crate::protocol::v2::TerminalInteractionNotification;
use crate::protocol::v2::ThreadItem;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem as CoreDynamicToolCallOutputContentItem;
use codex_protocol::protocol::EventMsg;

/// Build the v2 app-server notification that directly corresponds to a single core event.
///
/// This only covers the stateless event-to-notification projections that have a one-to-one
/// mapping. Callers remain responsible for any surrounding state checks or side effects before
/// invoking this helper.
pub fn item_event_to_server_notification(
    msg: EventMsg,
    thread_id: &str,
    turn_id: &str,
) -> ServerNotification {
    let thread_id = thread_id.to_string();
    let turn_id = turn_id.to_string();
    if let Some(projection) = collaboration_item_from_event(&msg) {
        return match projection.lifecycle {
            CollaborationItemLifecycle::Started { at_ms } => {
                ServerNotification::ItemStarted(ItemStartedNotification {
                    thread_id,
                    turn_id,
                    item: projection.item,
                    started_at_ms: at_ms,
                })
            }
            CollaborationItemLifecycle::Completed { at_ms } => {
                ServerNotification::ItemCompleted(ItemCompletedNotification {
                    thread_id,
                    turn_id,
                    item: projection.item,
                    completed_at_ms: at_ms,
                })
            }
        };
    }
    match msg {
        EventMsg::DynamicToolCallResponse(response) => {
            let status = if response.success {
                DynamicToolCallStatus::Completed
            } else {
                DynamicToolCallStatus::Failed
            };
            let duration_ms = i64::try_from(response.duration.as_millis()).ok();
            let item = ThreadItem::DynamicToolCall {
                id: response.call_id,
                namespace: response.namespace,
                tool: response.tool,
                arguments: response.arguments,
                status,
                content_items: Some(
                    response
                        .content_items
                        .into_iter()
                        .map(|item| match item {
                            CoreDynamicToolCallOutputContentItem::InputText { text } => {
                                DynamicToolCallOutputContentItem::InputText { text }
                            }
                            CoreDynamicToolCallOutputContentItem::InputImage { image_url } => {
                                DynamicToolCallOutputContentItem::InputImage { image_url }
                            }
                            CoreDynamicToolCallOutputContentItem::InputAudio { audio_url } => {
                                DynamicToolCallOutputContentItem::InputAudio { audio_url }
                            }
                        })
                        .collect(),
                ),
                success: Some(response.success),
                duration_ms,
            };
            ServerNotification::ItemCompleted(ItemCompletedNotification {
                thread_id,
                turn_id: response.turn_id,
                item,
                completed_at_ms: response.completed_at_ms,
            })
        }
        EventMsg::CollabAgentSpawnBegin(_)
        | EventMsg::CollabAgentSpawnEnd(_)
        | EventMsg::CollabAgentInteractionBegin(_)
        | EventMsg::CollabAgentInteractionEnd(_)
        | EventMsg::SubAgentActivity(_)
        | EventMsg::CollabWaitingBegin(_)
        | EventMsg::CollabWaitingEnd(_)
        | EventMsg::CollabCloseBegin(_)
        | EventMsg::CollabCloseEnd(_)
        | EventMsg::CollabResumeBegin(_)
        | EventMsg::CollabResumeEnd(_) => {
            unreachable!("collaboration events are projected before the general event match")
        }
        EventMsg::AgentMessageContentDelta(event) => {
            let codex_protocol::protocol::AgentMessageContentDeltaEvent { item_id, delta, .. } =
                event;
            ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
                thread_id,
                turn_id,
                item_id,
                delta,
            })
        }
        EventMsg::PlanDelta(event) => ServerNotification::PlanDelta(PlanDeltaNotification {
            thread_id,
            turn_id,
            item_id: event.item_id,
            delta: event.delta,
        }),
        EventMsg::ReasoningContentDelta(event) => {
            ServerNotification::ReasoningSummaryTextDelta(ReasoningSummaryTextDeltaNotification {
                thread_id,
                turn_id,
                item_id: event.item_id,
                delta: event.delta,
                summary_index: event.summary_index,
            })
        }
        EventMsg::ReasoningRawContentDelta(event) => {
            ServerNotification::ReasoningTextDelta(ReasoningTextDeltaNotification {
                thread_id,
                turn_id,
                item_id: event.item_id,
                delta: event.delta,
                content_index: event.content_index,
            })
        }
        EventMsg::AgentReasoningSectionBreak(event) => {
            ServerNotification::ReasoningSummaryPartAdded(ReasoningSummaryPartAddedNotification {
                thread_id,
                turn_id,
                item_id: event.item_id,
                summary_index: event.summary_index,
            })
        }
        EventMsg::ItemStarted(item_started_event) => {
            ServerNotification::ItemStarted(ItemStartedNotification {
                thread_id,
                turn_id,
                item: item_started_event.item.into(),
                started_at_ms: item_started_event.started_at_ms,
            })
        }
        EventMsg::ItemCompleted(item_completed_event) => {
            ServerNotification::ItemCompleted(ItemCompletedNotification {
                thread_id,
                turn_id,
                item: item_completed_event.item.into(),
                completed_at_ms: item_completed_event.completed_at_ms,
            })
        }
        EventMsg::PatchApplyUpdated(event) => {
            ServerNotification::FileChangePatchUpdated(FileChangePatchUpdatedNotification {
                thread_id,
                turn_id,
                item_id: event.call_id,
                changes: convert_patch_changes(&event.changes),
            })
        }
        EventMsg::ExecCommandBegin(exec_command_begin_event) => {
            ServerNotification::ItemStarted(ItemStartedNotification {
                thread_id,
                turn_id,
                item: build_command_execution_begin_item(&exec_command_begin_event),
                started_at_ms: exec_command_begin_event.started_at_ms,
            })
        }
        EventMsg::ExecCommandOutputDelta(exec_command_output_delta_event) => {
            let item_id = exec_command_output_delta_event.call_id;
            let delta = String::from_utf8_lossy(&exec_command_output_delta_event.chunk).to_string();
            ServerNotification::CommandExecutionOutputDelta(
                CommandExecutionOutputDeltaNotification {
                    thread_id,
                    turn_id,
                    item_id,
                    delta,
                },
            )
        }
        EventMsg::TerminalInteraction(terminal_event) => {
            ServerNotification::TerminalInteraction(TerminalInteractionNotification {
                thread_id,
                turn_id,
                item_id: terminal_event.call_id,
                process_id: terminal_event.process_id,
                stdin: terminal_event.stdin,
            })
        }
        EventMsg::ExecCommandEnd(exec_command_end_event) => {
            ServerNotification::ItemCompleted(ItemCompletedNotification {
                thread_id,
                turn_id,
                item: build_command_execution_end_item(&exec_command_end_event),
                completed_at_ms: exec_command_end_event.completed_at_ms,
            })
        }
        _ => unreachable!("unsupported item event"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_protocol::protocol::ExecCommandOutputDeltaEvent;
    use codex_protocol::protocol::ExecOutputStream;
    use pretty_assertions::assert_eq;

    fn assert_command_execution_output_delta_server_notification(
        notification: ServerNotification,
        expected: CommandExecutionOutputDeltaNotification,
    ) {
        match notification {
            ServerNotification::CommandExecutionOutputDelta(payload) => {
                assert_eq!(payload, expected)
            }
            other => panic!("expected command execution output delta, got {other:?}"),
        }
    }

    #[test]
    fn exec_command_output_delta_maps_to_command_execution_output_delta() {
        let notification = item_event_to_server_notification(
            EventMsg::ExecCommandOutputDelta(ExecCommandOutputDeltaEvent {
                call_id: "call-1".to_string(),
                stream: ExecOutputStream::Stdout,
                chunk: b"hello".to_vec(),
            }),
            "thread-1",
            "turn-1",
        );

        assert_command_execution_output_delta_server_notification(
            notification,
            CommandExecutionOutputDeltaNotification {
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                item_id: "call-1".to_string(),
                delta: "hello".to_string(),
            },
        );
    }
}
