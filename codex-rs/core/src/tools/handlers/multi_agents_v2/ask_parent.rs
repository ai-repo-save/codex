use super::*;
use crate::agent::AgentControl;
use crate::agent::control::ParentRequestOutcome;
use crate::agent_communication::AgentCommunicationContext;
use crate::agent_communication::AgentCommunicationKind;
use codex_agent_control::create_ask_parent_tool;
use codex_protocol::items::ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE;
use codex_protocol::items::AskParentMode;
use codex_tools::ToolSpec;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const DEFAULT_AUTHORITATIVE_TIMEOUT_MS: i64 = 240_000;

#[cfg(test)]
#[path = "ask_parent_tests.rs"]
mod tests;

#[derive(Default)]
pub(crate) struct Handler;

impl ToolExecutor<ToolInvocation> for Handler {
    fn tool_name(&self) -> ToolName {
        ToolName::plain("ask_parent")
    }

    fn spec(&self) -> ToolSpec {
        create_ask_parent_tool()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        Box::pin(self.handle_call(invocation))
    }
}

impl Handler {
    async fn handle_call(
        &self,
        invocation: ToolInvocation,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        let ToolInvocation {
            session,
            turn,
            payload,
            cancellation_token,
            call_id,
            ..
        } = invocation;
        let args: AskParentArgs = parse_arguments(&function_arguments(payload)?)?;
        let question = message_tool::message_content(args.question)?;
        let mode = args.mode.unwrap_or(AskParentMode::Authoritative);
        let timeout_ms = validate_timeout(&turn, args.timeout_ms, mode)?;
        let parent_thread_id = turn.parent_thread_id.ok_or_else(|| {
            FunctionCallError::RespondToModel(
                "ask_parent is only available to an agent with a direct parent".to_string(),
            )
        })?;
        let child_path = turn.session_source.get_agent_path().ok_or_else(|| {
            FunctionCallError::RespondToModel("current agent is missing an agent_path".to_string())
        })?;
        let parent = session
            .services
            .agent_control
            .ensure_agent_known(parent_thread_id)
            .map_err(|err| collab_agent_error(parent_thread_id, err))?;
        let parent_path = parent.agent_path.clone().ok_or_else(|| {
            FunctionCallError::RespondToModel("parent agent is missing an agent_path".to_string())
        })?;
        let parent_ref = codex_protocol::protocol::CollabAgentRef {
            thread_id: parent_thread_id,
            agent_nickname: parent.agent_nickname,
            agent_role: parent.agent_role,
        };
        if mode == AskParentMode::Consult {
            return self
                .handle_consult(
                    session,
                    turn,
                    call_id,
                    cancellation_token,
                    parent_thread_id,
                    parent_path,
                    parent_ref,
                    question,
                    timeout_ms,
                )
                .await;
        }
        let resume_config = build_agent_resume_config(turn.as_ref())?;
        emit_ask_parent_item(
            &session,
            &turn,
            &call_id,
            CollabAgentToolCallStatus::InProgress,
            AgentStatus::Running,
            parent_thread_id,
            parent_ref.clone(),
            &question,
            AskParentMode::Authoritative,
            /*snapshot_revision*/ None,
        )
        .await;
        if let Err(err) = session
            .services
            .agent_control
            .ensure_v2_agent_loaded(resume_config, parent_thread_id)
            .await
        {
            emit_ask_parent_item(
                &session,
                &turn,
                &call_id,
                CollabAgentToolCallStatus::Failed,
                AgentStatus::NotFound,
                parent_thread_id,
                parent_ref,
                &question,
                AskParentMode::Authoritative,
                /*snapshot_revision*/ None,
            )
            .await;
            return Err(collab_agent_error(parent_thread_id, err));
        }

        let (request_id, receiver) = session
            .services
            .agent_control
            .register_parent_request(session.thread_id, parent_thread_id);
        let mut registration = ParentRequestRegistration::new(
            session.services.agent_control.clone(),
            request_id.clone(),
        );
        let content = format!(
            "Parent decision request `{request_id}` from {child_path}.\n\n{question}\n\nReply with `send_message` targeting `{child_path}` and `in_reply_to: \"{request_id}\"`."
        );
        let communication = communication_from_tool_message(
            child_path,
            parent_path.clone(),
            content,
            turn.config.multi_agent_v2.encrypt_messages,
        );
        let context = AgentCommunicationContext::new(
            AgentCommunicationKind::ParentRequest,
            session.thread_id,
        );
        if let Err(err) = session
            .services
            .agent_control
            .send_inter_agent_communication(parent_thread_id, communication, context)
            .await
        {
            emit_ask_parent_item(
                &session,
                &turn,
                &call_id,
                CollabAgentToolCallStatus::Failed,
                AgentStatus::NotFound,
                parent_thread_id,
                parent_ref,
                &question,
                AskParentMode::Authoritative,
                /*snapshot_revision*/ None,
            )
            .await;
            return Err(collab_agent_error(parent_thread_id, err));
        }

        let outcome = wait_for_parent_outcome(
            receiver,
            &cancellation_token,
            Duration::from_millis(timeout_ms as u64),
            &session.services.agent_control,
            &request_id,
        )
        .await;
        let (status, answer) = match outcome {
            ParentWaitResult::Answered(answer) => {
                registration.disarm();
                (AskParentStatus::Answered, Some(answer))
            }
            ParentWaitResult::ParentUnavailable => {
                registration.disarm();
                (AskParentStatus::ParentUnavailable, None)
            }
            ParentWaitResult::Cancelled => {
                registration.disarm();
                emit_ask_parent_item(
                    &session,
                    &turn,
                    &call_id,
                    CollabAgentToolCallStatus::Failed,
                    AgentStatus::NotFound,
                    parent_thread_id,
                    parent_ref,
                    &question,
                    AskParentMode::Authoritative,
                    /*snapshot_revision*/ None,
                )
                .await;
                return Err(FunctionCallError::RespondToModel(
                    "ask_parent was cancelled".to_string(),
                ));
            }
            ParentWaitResult::TimedOut => {
                registration.disarm();
                (AskParentStatus::TimedOut, None)
            }
        };
        emit_ask_parent_item(
            &session,
            &turn,
            &call_id,
            CollabAgentToolCallStatus::Completed,
            match &status {
                AskParentStatus::Answered => AgentStatus::Completed(answer.clone()),
                AskParentStatus::TimedOut => AgentStatus::Interrupted,
                AskParentStatus::ParentUnavailable => AgentStatus::NotFound,
                AskParentStatus::RequiresAuthoritativeParent => AgentStatus::Interrupted,
            },
            parent_thread_id,
            parent_ref,
            &question,
            AskParentMode::Authoritative,
            /*snapshot_revision*/ None,
        )
        .await;
        Ok(boxed_tool_output(AskParentResult {
            request_id,
            parent_thread_id,
            parent_path,
            status,
            answer,
            mode: AskParentMode::Authoritative,
            advisory: None,
            snapshot_revision: None,
            snapshot_may_be_stale: None,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_consult(
        &self,
        session: std::sync::Arc<crate::session::session::Session>,
        turn: std::sync::Arc<crate::session::turn_context::TurnContext>,
        call_id: String,
        cancellation_token: CancellationToken,
        parent_thread_id: codex_protocol::ThreadId,
        parent_path: AgentPath,
        parent_ref: codex_protocol::protocol::CollabAgentRef,
        question: String,
        timeout_ms: i64,
    ) -> Result<Box<dyn crate::tools::context::ToolOutput>, FunctionCallError> {
        emit_ask_parent_item(
            &session,
            &turn,
            &call_id,
            CollabAgentToolCallStatus::InProgress,
            AgentStatus::Running,
            parent_thread_id,
            parent_ref.clone(),
            &question,
            AskParentMode::Consult,
            /*snapshot_revision*/ None,
        )
        .await;
        let loaded_parent = match session
            .services
            .agent_control
            .loaded_agent_consult_snapshot(parent_thread_id)
            .await
        {
            Ok(loaded_parent) => loaded_parent,
            Err(_) => {
                emit_ask_parent_item(
                    &session,
                    &turn,
                    &call_id,
                    CollabAgentToolCallStatus::Completed,
                    AgentStatus::NotFound,
                    parent_thread_id,
                    parent_ref,
                    &question,
                    AskParentMode::Consult,
                    /*snapshot_revision*/ None,
                )
                .await;
                return Ok(boxed_tool_output(AskParentResult {
                    request_id: call_id,
                    parent_thread_id,
                    parent_path,
                    status: AskParentStatus::ParentUnavailable,
                    answer: None,
                    mode: AskParentMode::Consult,
                    advisory: None,
                    snapshot_revision: None,
                    snapshot_may_be_stale: Some(true),
                }));
            }
        };
        let (outcome, snapshot_revision) = match super::consult::consult_parent(
            loaded_parent,
            question.clone(),
            &cancellation_token,
            Duration::from_millis(timeout_ms as u64),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                let error_message = error.to_string();
                emit_ask_parent_item(
                    &session,
                    &turn,
                    &call_id,
                    CollabAgentToolCallStatus::Failed,
                    AgentStatus::Errored(error_message),
                    parent_thread_id,
                    parent_ref,
                    &question,
                    AskParentMode::Consult,
                    /*snapshot_revision*/ None,
                )
                .await;
                return Err(error);
            }
        };
        let (status, advisory, parent_status) = match outcome {
            super::consult::ConsultRunOutcome::Completed(response) => match response.kind {
                super::consult::ConsultResponseKind::Advisory => (
                    AskParentStatus::Answered,
                    Some(response.advisory.clone()),
                    AgentStatus::Completed(Some(response.advisory)),
                ),
                super::consult::ConsultResponseKind::RequiresAuthoritativeParent => (
                    AskParentStatus::RequiresAuthoritativeParent,
                    Some(response.advisory),
                    AgentStatus::Completed(Some(
                        ASK_PARENT_REQUIRES_AUTHORITATIVE_MESSAGE.to_string(),
                    )),
                ),
            },
            super::consult::ConsultRunOutcome::TimedOut => {
                (AskParentStatus::TimedOut, None, AgentStatus::Interrupted)
            }
            super::consult::ConsultRunOutcome::Cancelled => {
                emit_ask_parent_item(
                    &session,
                    &turn,
                    &call_id,
                    CollabAgentToolCallStatus::Failed,
                    AgentStatus::Errored("ask_parent consult was cancelled".to_string()),
                    parent_thread_id,
                    parent_ref,
                    &question,
                    AskParentMode::Consult,
                    Some(snapshot_revision),
                )
                .await;
                return Err(FunctionCallError::RespondToModel(
                    "ask_parent consult was cancelled".to_string(),
                ));
            }
        };
        emit_ask_parent_item(
            &session,
            &turn,
            &call_id,
            CollabAgentToolCallStatus::Completed,
            parent_status,
            parent_thread_id,
            parent_ref,
            &question,
            AskParentMode::Consult,
            Some(snapshot_revision.clone()),
        )
        .await;
        Ok(boxed_tool_output(AskParentResult {
            request_id: call_id,
            parent_thread_id,
            parent_path,
            status,
            answer: None,
            mode: AskParentMode::Consult,
            advisory,
            snapshot_revision: Some(snapshot_revision),
            snapshot_may_be_stale: Some(true),
        }))
    }
}

async fn wait_for_parent_outcome(
    mut receiver: oneshot::Receiver<ParentRequestOutcome>,
    cancellation_token: &CancellationToken,
    timeout: Duration,
    control: &AgentControl,
    request_id: &str,
) -> ParentWaitResult {
    tokio::select! {
        biased;
        result = &mut receiver => parent_wait_result(result),
        () = cancellation_token.cancelled() => {
            if control.cancel_parent_request(request_id) {
                ParentWaitResult::Cancelled
            } else {
                parent_wait_result(receiver.await)
            }
        },
        () = tokio::time::sleep(timeout) => {
            if control.cancel_parent_request(request_id) {
                ParentWaitResult::TimedOut
            } else {
                parent_wait_result(receiver.await)
            }
        },
    }
}

fn parent_wait_result(
    outcome: Result<ParentRequestOutcome, oneshot::error::RecvError>,
) -> ParentWaitResult {
    match outcome {
        Ok(ParentRequestOutcome::Answered {
            answer,
            acknowledgment,
        }) => {
            let _ = acknowledgment.send(());
            ParentWaitResult::Answered(answer)
        }
        Ok(ParentRequestOutcome::ParentUnavailable) | Err(_) => ParentWaitResult::ParentUnavailable,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ParentWaitResult {
    Answered(String),
    TimedOut,
    Cancelled,
    ParentUnavailable,
}

async fn emit_ask_parent_item(
    session: &crate::session::session::Session,
    turn: &crate::session::turn_context::TurnContext,
    call_id: &str,
    status: CollabAgentToolCallStatus,
    parent_status: AgentStatus,
    parent_thread_id: codex_protocol::ThreadId,
    parent: codex_protocol::protocol::CollabAgentRef,
    question: &str,
    mode: AskParentMode,
    snapshot_revision: Option<String>,
) {
    let item = CollabAgentToolCallItem {
        id: call_id.to_string(),
        tool: CollabAgentTool::AskParent,
        status,
        sender_thread_id: session.thread_id,
        receiver_thread_ids: vec![parent_thread_id],
        receiver_agents: vec![parent],
        prompt: Some(question.to_string()),
        model: None,
        reasoning_effort: None,
        service_tier: None,
        context_inheritance: None,
        mode: Some(mode),
        snapshot_revision,
        agents_states: HashMap::from([(parent_thread_id, parent_status)]),
    };
    if status == CollabAgentToolCallStatus::InProgress {
        session
            .emit_turn_item_started(turn, &TurnItem::CollabAgentToolCall(item))
            .await;
    } else {
        session
            .emit_turn_item_completed(turn, TurnItem::CollabAgentToolCall(item))
            .await;
    }
}

impl CoreToolRuntime for Handler {
    fn matches_kind(&self, payload: &ToolPayload) -> bool {
        matches!(payload, ToolPayload::Function { .. })
    }
}

struct ParentRequestRegistration {
    control: AgentControl,
    request_id: Option<String>,
}

impl ParentRequestRegistration {
    fn new(control: AgentControl, request_id: String) -> Self {
        Self {
            control,
            request_id: Some(request_id),
        }
    }

    fn disarm(&mut self) {
        self.request_id = None;
    }
}

impl Drop for ParentRequestRegistration {
    fn drop(&mut self) {
        if let Some(request_id) = self.request_id.as_deref() {
            self.control.cancel_parent_request(request_id);
        }
    }
}

fn validate_timeout(
    turn: &crate::session::turn_context::TurnContext,
    timeout_ms: Option<i64>,
    mode: AskParentMode,
) -> Result<i64, FunctionCallError> {
    let min = turn.config.multi_agent_v2.min_wait_timeout_ms;
    let max = turn.config.multi_agent_v2.max_wait_timeout_ms;
    let Some(timeout_ms) = timeout_ms else {
        return Ok(default_timeout_ms(
            mode,
            turn.config.multi_agent_v2.default_wait_timeout_ms,
            min,
            max,
        ));
    };
    if timeout_ms < min {
        return Err(FunctionCallError::RespondToModel(format!(
            "timeout_ms must be at least {min}"
        )));
    }
    if timeout_ms > max {
        return Err(FunctionCallError::RespondToModel(format!(
            "timeout_ms must be at most {max}"
        )));
    }
    Ok(timeout_ms)
}

fn default_timeout_ms(
    mode: AskParentMode,
    configured_wait_timeout_ms: i64,
    min_timeout_ms: i64,
    max_timeout_ms: i64,
) -> i64 {
    let timeout_ms = match mode {
        AskParentMode::Authoritative => DEFAULT_AUTHORITATIVE_TIMEOUT_MS,
        AskParentMode::Consult => configured_wait_timeout_ms,
    };
    timeout_ms.clamp(min_timeout_ms, max_timeout_ms)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AskParentArgs {
    question: String,
    timeout_ms: Option<i64>,
    mode: Option<AskParentMode>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AskParentStatus {
    Answered,
    TimedOut,
    ParentUnavailable,
    RequiresAuthoritativeParent,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct AskParentResult {
    pub(crate) request_id: String,
    pub(crate) parent_thread_id: codex_protocol::ThreadId,
    pub(crate) parent_path: AgentPath,
    pub(crate) status: AskParentStatus,
    pub(crate) answer: Option<String>,
    pub(crate) mode: AskParentMode,
    pub(crate) advisory: Option<String>,
    pub(crate) snapshot_revision: Option<String>,
    pub(crate) snapshot_may_be_stale: Option<bool>,
}

impl ToolOutput for AskParentResult {
    fn log_preview(&self) -> String {
        tool_output_json_text(self, "ask_parent")
    }

    fn success_for_logging(&self) -> bool {
        true
    }

    fn to_response_item(&self, call_id: &str, payload: &ToolPayload) -> ResponseInputItem {
        tool_output_response_item(call_id, payload, self, /*success*/ None, "ask_parent")
    }

    fn code_mode_result(&self, _payload: &ToolPayload) -> JsonValue {
        tool_output_code_mode_result(self, "ask_parent")
    }
}
