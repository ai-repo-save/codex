use crate::protocol::v2::AskParentMode;
use crate::protocol::v2::CollabAgentState;
use crate::protocol::v2::CollabAgentTool;
use crate::protocol::v2::CollabAgentToolCallStatus;
use crate::protocol::v2::SpawnContextInheritance;
use crate::protocol::v2::ThreadItem;
use codex_protocol::items::CollabAgentToolCallItem;
use codex_protocol::items::SubAgentActivityItem;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::EventMsg;
use std::collections::HashMap;

pub(crate) enum CollaborationItemLifecycle {
    Started { at_ms: i64 },
    Completed { at_ms: i64 },
}

pub(crate) struct CollaborationItemProjection {
    pub item: ThreadItem,
    pub lifecycle: CollaborationItemLifecycle,
}

pub(crate) struct SpawnRuntimeMetadata {
    model: Option<String>,
    reasoning_effort: Option<ReasoningEffort>,
    service_tier: Option<String>,
    context_inheritance: Option<SpawnContextInheritance>,
}

struct CollabAgentToolCallPayload {
    id: String,
    tool: CollabAgentTool,
    status: CollabAgentToolCallStatus,
    sender_thread_id: String,
    receiver_thread_ids: Vec<String>,
    prompt: Option<String>,
    runtime: Option<SpawnRuntimeMetadata>,
    mode: Option<AskParentMode>,
    snapshot_revision: Option<String>,
    agents_states: HashMap<String, CollabAgentState>,
}

impl CollabAgentToolCallPayload {
    fn into_thread_item(self) -> ThreadItem {
        let SpawnRuntimeMetadata {
            model,
            reasoning_effort,
            service_tier,
            context_inheritance,
        } = self.runtime.unwrap_or(SpawnRuntimeMetadata {
            model: None,
            reasoning_effort: None,
            service_tier: None,
            context_inheritance: None,
        });
        ThreadItem::CollabAgentToolCall {
            id: self.id,
            tool: self.tool,
            status: self.status,
            sender_thread_id: self.sender_thread_id,
            receiver_thread_ids: self.receiver_thread_ids,
            prompt: self.prompt,
            model,
            reasoning_effort,
            service_tier,
            context_inheritance,
            mode: self.mode,
            snapshot_revision: self.snapshot_revision,
            agents_states: self.agents_states,
        }
    }
}

struct SubAgentActivityPayload {
    id: String,
    kind: crate::protocol::v2::SubAgentActivityKind,
    agent_thread_id: String,
    agent_path: String,
    operation: Option<crate::protocol::v2::SubAgentActivityOperation>,
    outcome: Option<crate::protocol::v2::SubAgentActivityOutcome>,
    runtime: SpawnRuntimeMetadata,
}

impl SubAgentActivityPayload {
    fn into_thread_item(self) -> ThreadItem {
        ThreadItem::SubAgentActivity {
            id: self.id,
            kind: self.kind,
            agent_thread_id: self.agent_thread_id,
            agent_path: self.agent_path,
            operation: self.operation,
            outcome: self.outcome,
            model: self.runtime.model,
            reasoning_effort: self.runtime.reasoning_effort,
            service_tier: self.runtime.service_tier,
            context_inheritance: self.runtime.context_inheritance,
        }
    }
}

fn terminal_status(status: &AgentStatus) -> CollabAgentToolCallStatus {
    match status {
        AgentStatus::Errored(_) | AgentStatus::NotFound => CollabAgentToolCallStatus::Failed,
        _ => CollabAgentToolCallStatus::Completed,
    }
}

fn projection(
    item: CollabAgentToolCallPayload,
    lifecycle: CollaborationItemLifecycle,
) -> CollaborationItemProjection {
    CollaborationItemProjection {
        item: item.into_thread_item(),
        lifecycle,
    }
}

pub(crate) fn collaboration_item_from_event(
    event: &EventMsg,
) -> Option<CollaborationItemProjection> {
    let projection = match event {
        EventMsg::CollabAgentSpawnBegin(event) => projection(
            CollabAgentToolCallPayload {
                id: event.call_id.clone(),
                tool: CollabAgentTool::SpawnAgent,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: Vec::new(),
                prompt: Some(event.prompt.clone()),
                runtime: Some(SpawnRuntimeMetadata {
                    model: Some(event.model.clone()),
                    reasoning_effort: Some(event.reasoning_effort.clone()),
                    service_tier: event.service_tier.clone(),
                    context_inheritance: event.context_inheritance.clone().map(Into::into),
                }),
                mode: None,
                snapshot_revision: None,
                agents_states: HashMap::new(),
            },
            CollaborationItemLifecycle::Started {
                at_ms: event.started_at_ms,
            },
        ),
        EventMsg::CollabAgentSpawnEnd(event) => {
            let (receiver_thread_ids, agents_states) = match &event.new_thread_id {
                Some(id) => {
                    let receiver_id = id.to_string();
                    (
                        vec![receiver_id.clone()],
                        [(receiver_id, CollabAgentState::from(event.status.clone()))]
                            .into_iter()
                            .collect(),
                    )
                }
                None => (Vec::new(), HashMap::new()),
            };
            let status = if receiver_thread_ids.is_empty() {
                CollabAgentToolCallStatus::Failed
            } else {
                terminal_status(&event.status)
            };
            projection(
                CollabAgentToolCallPayload {
                    id: event.call_id.clone(),
                    tool: CollabAgentTool::SpawnAgent,
                    status,
                    sender_thread_id: event.sender_thread_id.to_string(),
                    receiver_thread_ids,
                    prompt: Some(event.prompt.clone()),
                    runtime: Some(SpawnRuntimeMetadata {
                        model: Some(event.model.clone()),
                        reasoning_effort: Some(event.reasoning_effort.clone()),
                        service_tier: event.service_tier.clone(),
                        context_inheritance: event.context_inheritance.clone().map(Into::into),
                    }),
                    mode: None,
                    snapshot_revision: None,
                    agents_states,
                },
                CollaborationItemLifecycle::Completed {
                    at_ms: event.completed_at_ms,
                },
            )
        }
        EventMsg::CollabAgentInteractionBegin(event) => projection(
            CollabAgentToolCallPayload {
                id: event.call_id.clone(),
                tool: CollabAgentTool::SendInput,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: vec![event.receiver_thread_id.to_string()],
                prompt: Some(event.prompt.clone()),
                runtime: None,
                mode: None,
                snapshot_revision: None,
                agents_states: HashMap::new(),
            },
            CollaborationItemLifecycle::Started {
                at_ms: event.started_at_ms,
            },
        ),
        EventMsg::CollabAgentInteractionEnd(event) => {
            let receiver_id = event.receiver_thread_id.to_string();
            projection(
                CollabAgentToolCallPayload {
                    id: event.call_id.clone(),
                    tool: CollabAgentTool::SendInput,
                    status: terminal_status(&event.status),
                    sender_thread_id: event.sender_thread_id.to_string(),
                    receiver_thread_ids: vec![receiver_id.clone()],
                    prompt: Some(event.prompt.clone()),
                    runtime: None,
                    mode: None,
                    snapshot_revision: None,
                    agents_states: [(receiver_id, CollabAgentState::from(event.status.clone()))]
                        .into_iter()
                        .collect(),
                },
                CollaborationItemLifecycle::Completed {
                    at_ms: event.completed_at_ms,
                },
            )
        }
        EventMsg::SubAgentActivity(event) => CollaborationItemProjection {
            item: SubAgentActivityPayload {
                id: event.event_id.clone(),
                kind: event.kind.into(),
                agent_thread_id: event.agent_thread_id.to_string(),
                agent_path: String::from(event.agent_path.clone()),
                operation: event.operation.map(Into::into),
                outcome: event.outcome.map(Into::into),
                runtime: SpawnRuntimeMetadata {
                    model: event.model.clone(),
                    reasoning_effort: event.reasoning_effort.clone(),
                    service_tier: event.service_tier.clone(),
                    context_inheritance: event.context_inheritance.clone().map(Into::into),
                },
            }
            .into_thread_item(),
            lifecycle: CollaborationItemLifecycle::Completed {
                at_ms: event.occurred_at_ms,
            },
        },
        EventMsg::CollabWaitingBegin(event) => projection(
            CollabAgentToolCallPayload {
                id: event.call_id.clone(),
                tool: CollabAgentTool::Wait,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: event
                    .receiver_thread_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                prompt: None,
                runtime: None,
                mode: None,
                snapshot_revision: None,
                agents_states: HashMap::new(),
            },
            CollaborationItemLifecycle::Started {
                at_ms: event.started_at_ms,
            },
        ),
        EventMsg::CollabWaitingEnd(event) => {
            let status = if event
                .statuses
                .values()
                .any(|status| matches!(status, AgentStatus::Errored(_) | AgentStatus::NotFound))
            {
                CollabAgentToolCallStatus::Failed
            } else {
                CollabAgentToolCallStatus::Completed
            };
            let mut receiver_thread_ids: Vec<String> =
                event.statuses.keys().map(ToString::to_string).collect();
            receiver_thread_ids.sort();
            let agents_states = event
                .statuses
                .iter()
                .map(|(id, status)| (id.to_string(), CollabAgentState::from(status.clone())))
                .collect();
            projection(
                CollabAgentToolCallPayload {
                    id: event.call_id.clone(),
                    tool: CollabAgentTool::Wait,
                    status,
                    sender_thread_id: event.sender_thread_id.to_string(),
                    receiver_thread_ids,
                    prompt: None,
                    runtime: None,
                    mode: None,
                    snapshot_revision: None,
                    agents_states,
                },
                CollaborationItemLifecycle::Completed {
                    at_ms: event.completed_at_ms,
                },
            )
        }
        EventMsg::CollabCloseBegin(event) => projection(
            CollabAgentToolCallPayload {
                id: event.call_id.clone(),
                tool: CollabAgentTool::CloseAgent,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: vec![event.receiver_thread_id.to_string()],
                prompt: None,
                runtime: None,
                mode: None,
                snapshot_revision: None,
                agents_states: HashMap::new(),
            },
            CollaborationItemLifecycle::Started {
                at_ms: event.started_at_ms,
            },
        ),
        EventMsg::CollabCloseEnd(event) => terminal_projection(
            event.call_id.clone(),
            CollabAgentTool::CloseAgent,
            event.sender_thread_id.to_string(),
            event.receiver_thread_id.to_string(),
            &event.status,
            event.completed_at_ms,
        ),
        EventMsg::CollabResumeBegin(event) => projection(
            CollabAgentToolCallPayload {
                id: event.call_id.clone(),
                tool: CollabAgentTool::ResumeAgent,
                status: CollabAgentToolCallStatus::InProgress,
                sender_thread_id: event.sender_thread_id.to_string(),
                receiver_thread_ids: vec![event.receiver_thread_id.to_string()],
                prompt: None,
                runtime: None,
                mode: None,
                snapshot_revision: None,
                agents_states: HashMap::new(),
            },
            CollaborationItemLifecycle::Started {
                at_ms: event.started_at_ms,
            },
        ),
        EventMsg::CollabResumeEnd(event) => terminal_projection(
            event.call_id.clone(),
            CollabAgentTool::ResumeAgent,
            event.sender_thread_id.to_string(),
            event.receiver_thread_id.to_string(),
            &event.status,
            event.completed_at_ms,
        ),
        _ => return None,
    };
    Some(projection)
}

fn terminal_projection(
    id: String,
    tool: CollabAgentTool,
    sender_thread_id: String,
    receiver_thread_id: String,
    status: &AgentStatus,
    completed_at_ms: i64,
) -> CollaborationItemProjection {
    projection(
        CollabAgentToolCallPayload {
            id,
            tool,
            status: terminal_status(status),
            sender_thread_id,
            receiver_thread_ids: vec![receiver_thread_id.clone()],
            prompt: None,
            runtime: None,
            mode: None,
            snapshot_revision: None,
            agents_states: [(receiver_thread_id, CollabAgentState::from(status.clone()))]
                .into_iter()
                .collect(),
        },
        CollaborationItemLifecycle::Completed {
            at_ms: completed_at_ms,
        },
    )
}

pub(crate) fn collab_agent_tool_call_from_core(call: CollabAgentToolCallItem) -> ThreadItem {
    CollabAgentToolCallPayload {
        id: call.id,
        tool: call.tool.into(),
        status: call.status.into(),
        sender_thread_id: call.sender_thread_id.to_string(),
        receiver_thread_ids: call
            .receiver_thread_ids
            .into_iter()
            .map(String::from)
            .collect(),
        prompt: call.prompt,
        runtime: Some(SpawnRuntimeMetadata {
            model: call.model,
            reasoning_effort: call.reasoning_effort,
            service_tier: call.service_tier,
            context_inheritance: call.context_inheritance.map(Into::into),
        }),
        mode: call.mode.map(AskParentMode::from),
        snapshot_revision: call.snapshot_revision,
        agents_states: call
            .agents_states
            .into_iter()
            .map(|(thread_id, status)| (thread_id.to_string(), status.into()))
            .collect(),
    }
    .into_thread_item()
}

pub(crate) fn sub_agent_activity_from_core(activity: SubAgentActivityItem) -> ThreadItem {
    SubAgentActivityPayload {
        id: activity.id,
        kind: activity.kind.into(),
        agent_thread_id: activity.agent_thread_id.to_string(),
        agent_path: String::from(activity.agent_path),
        operation: activity.operation.map(Into::into),
        outcome: activity.outcome.map(Into::into),
        runtime: SpawnRuntimeMetadata {
            model: activity.model,
            reasoning_effort: activity.reasoning_effort,
            service_tier: activity.service_tier,
            context_inheritance: activity.context_inheritance.map(Into::into),
        },
    }
    .into_thread_item()
}

#[cfg(test)]
#[path = "collaboration_items_tests.rs"]
mod tests;
