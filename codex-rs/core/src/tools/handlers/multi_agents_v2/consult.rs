use super::*;
use crate::agent::control::LoadedAgentConsult;
use crate::context::ConsultParentContext;
use crate::context::ContextualUserFragment;
use crate::session::Codex;
use crate::session::CodexSpawnArgs;
use crate::session::CodexSpawnOk;
use crate::session::turn_context::ToolExecutionMode;
use codex_extension_api::ExtensionDataInit;
use codex_extension_api::LoadedUserInstructions;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::InitialHistory;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::RolloutItem;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_protocol::user_input::UserInput;
use serde::Deserialize;
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const CONSULT_RESPONDER_SOURCE: &str = "ask_parent_consult";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ConsultResponseKind {
    Advisory,
    RequiresAuthoritativeParent,
}

#[derive(Debug, Deserialize)]
pub(super) struct ConsultResponse {
    pub(super) kind: ConsultResponseKind,
    pub(super) advisory: String,
}

pub(super) enum ConsultRunOutcome {
    Completed(ConsultResponse),
    TimedOut,
    Cancelled,
}

struct ConsultResponderCleanup {
    codex: Option<Codex>,
}

impl ConsultResponderCleanup {
    fn new(codex: Codex) -> Self {
        Self { codex: Some(codex) }
    }

    async fn shutdown(&mut self) {
        if let Some(codex) = self.codex.take() {
            let _ = codex.shutdown_and_wait().await;
        }
    }
}

impl Drop for ConsultResponderCleanup {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            return;
        }
        let Some(codex) = self.codex.take() else {
            return;
        };
        drop(tokio::spawn(async move {
            let _ = codex.shutdown_and_wait().await;
        }));
    }
}

pub(super) async fn consult_parent(
    loaded_parent: LoadedAgentConsult,
    question: String,
    cancellation_token: &CancellationToken,
    timeout: Duration,
) -> Result<(ConsultRunOutcome, String), FunctionCallError> {
    let LoadedAgentConsult {
        session: parent_session,
        mut snapshot,
    } = loaded_parent;
    let revision = snapshot.revision.clone();
    let deadline = Instant::now() + timeout;
    snapshot
        .history
        .push(RolloutItem::ResponseItem(ContextualUserFragment::into(
            ConsultParentContext,
        )));
    let instructions = match wait_for_consult_stage(
        parent_session.user_instructions(),
        cancellation_token,
        deadline,
    )
    .await
    {
        ConsultStage::Completed(instructions) => instructions,
        ConsultStage::TimedOut => return Ok((ConsultRunOutcome::TimedOut, revision)),
        ConsultStage::Cancelled => return Ok((ConsultRunOutcome::Cancelled, revision)),
    };
    let user_instructions = LoadedUserInstructions {
        instructions,
        warnings: Vec::new(),
    };
    let spawned = wait_for_consult_stage(
        Codex::spawn(CodexSpawnArgs {
            config: snapshot.config,
            tool_execution_mode: ToolExecutionMode::ConsultNoLocalTools,
            allow_provider_model_fallback: false,
            user_instructions,
            installation_id: parent_session.installation_id.clone(),
            auth_manager: Arc::clone(&parent_session.services.auth_manager),
            models_manager: Arc::clone(&parent_session.services.models_manager),
            environment_manager: parent_session
                .services
                .turn_environments
                .environment_manager(),
            skills_service: Arc::clone(&parent_session.services.skills_service),
            plugins_manager: Arc::clone(&parent_session.services.plugins_manager),
            mcp_manager: Arc::clone(&parent_session.services.mcp_manager),
            code_mode_session_provider: parent_session
                .services
                .code_mode_service
                .session_provider(),
            extensions: Arc::clone(&parent_session.services.extensions),
            conversation_history: InitialHistory::Forked(snapshot.history),
            requested_history_mode: None,
            session_source: SessionSource::SubAgent(SubAgentSource::Other(
                CONSULT_RESPONDER_SOURCE.to_string(),
            )),
            forked_from_thread_id: None,
            parent_thread_id: snapshot.parent_thread_id,
            thread_source: None,
            originator: snapshot.originator,
            agent_control: parent_session.services.agent_control.clone(),
            dynamic_tools: snapshot.dynamic_tools,
            metrics_service_name: None,
            inherited_exec_policy: None,
            inherited_environments: None,
            parent_rollout_thread_trace: codex_rollout_trace::ThreadTraceContext::disabled(),
            user_shell_override: None,
            parent_trace: None,
            environment_selections: snapshot.environments,
            thread_extension_init: ExtensionDataInit::default(),
            supports_openai_form_elicitation: parent_session
                .services
                .supports_openai_form_elicitation
                .load(Ordering::Relaxed),
            analytics_events_client: None,
            thread_store: Arc::clone(&parent_session.services.thread_store),
            attestation_provider: parent_session.services.attestation_provider.clone(),
            external_time_provider: Some(Arc::clone(&parent_session.services.time_provider)),
            inherited_multi_agent_version: snapshot.multi_agent_version,
            prompt_cache_key_override: Some(snapshot.prompt_cache_key),
        }),
        cancellation_token,
        deadline,
    )
    .await;
    let CodexSpawnOk { codex, .. } = match spawned {
        ConsultStage::Completed(Ok(spawned)) => spawned,
        ConsultStage::Completed(Err(error)) => {
            return Err(FunctionCallError::RespondToModel(format!(
                "consult responder failed to start: {error}"
            )));
        }
        ConsultStage::TimedOut => return Ok((ConsultRunOutcome::TimedOut, revision)),
        ConsultStage::Cancelled => return Ok((ConsultRunOutcome::Cancelled, revision)),
    };
    let mut cleanup = ConsultResponderCleanup::new(codex);
    let configured = match wait_for_consult_stage(
        cleanup
            .codex
            .as_ref()
            .expect("consult responder exists until cleanup")
            .next_event(),
        cancellation_token,
        deadline,
    )
    .await
    {
        ConsultStage::Completed(Ok(configured)) => configured,
        ConsultStage::Completed(Err(error)) => {
            cleanup.shutdown().await;
            return Err(FunctionCallError::RespondToModel(format!(
                "consult responder failed to configure: {error}"
            )));
        }
        ConsultStage::TimedOut => {
            cleanup.shutdown().await;
            return Ok((ConsultRunOutcome::TimedOut, revision));
        }
        ConsultStage::Cancelled => {
            cleanup.shutdown().await;
            return Ok((ConsultRunOutcome::Cancelled, revision));
        }
    };
    if !matches!(configured.msg, EventMsg::SessionConfigured(_)) {
        cleanup.shutdown().await;
        return Err(FunctionCallError::RespondToModel(
            "consult responder did not emit session configuration first".to_string(),
        ));
    }

    let codex = cleanup
        .codex
        .as_ref()
        .expect("consult responder exists until cleanup");
    let submitted = wait_for_consult_stage(
        codex.submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: question,
                text_elements: Vec::new(),
            }],
            final_output_json_schema: Some(consult_output_schema()),
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: Default::default(),
        }),
        cancellation_token,
        deadline,
    )
    .await;
    let expected_turn_id = match submitted {
        ConsultStage::Completed(Ok(expected_turn_id)) => expected_turn_id,
        ConsultStage::Completed(Err(error)) => {
            cleanup.shutdown().await;
            return Err(FunctionCallError::RespondToModel(format!(
                "consult responder failed to submit: {error}"
            )));
        }
        ConsultStage::TimedOut => {
            cleanup.shutdown().await;
            return Ok((ConsultRunOutcome::TimedOut, revision));
        }
        ConsultStage::Cancelled => {
            cleanup.shutdown().await;
            return Ok((ConsultRunOutcome::Cancelled, revision));
        }
    };

    let outcome = match wait_for_consult_stage(
        wait_for_consult_response(codex, &expected_turn_id),
        cancellation_token,
        deadline,
    )
    .await
    {
        ConsultStage::Completed(Ok(response)) => ConsultRunOutcome::Completed(response),
        ConsultStage::Completed(Err(error)) => {
            cleanup.shutdown().await;
            return Err(error);
        }
        ConsultStage::TimedOut => ConsultRunOutcome::TimedOut,
        ConsultStage::Cancelled => ConsultRunOutcome::Cancelled,
    };
    cleanup.shutdown().await;
    Ok((outcome, revision))
}

enum ConsultStage<T> {
    Completed(T),
    TimedOut,
    Cancelled,
}

async fn wait_for_consult_stage<T>(
    operation: impl Future<Output = T>,
    cancellation_token: &CancellationToken,
    deadline: Instant,
) -> ConsultStage<T> {
    tokio::select! {
        biased;
        result = operation => ConsultStage::Completed(result),
        () = cancellation_token.cancelled() => ConsultStage::Cancelled,
        () = tokio::time::sleep_until(deadline) => ConsultStage::TimedOut,
    }
}

async fn wait_for_consult_response(
    codex: &Codex,
    expected_turn_id: &str,
) -> Result<ConsultResponse, FunctionCallError> {
    loop {
        let event = codex.next_event().await.map_err(|error| {
            FunctionCallError::RespondToModel(format!(
                "consult responder ended unexpectedly: {error}"
            ))
        })?;
        if event.id != expected_turn_id {
            continue;
        }
        match event.msg {
            EventMsg::TurnComplete(turn_complete) if turn_complete.turn_id == expected_turn_id => {
                let response = turn_complete.last_agent_message.ok_or_else(|| {
                    FunctionCallError::RespondToModel(
                        "consult responder completed without an advisory".to_string(),
                    )
                })?;
                return serde_json::from_str(&response).map_err(|error| {
                    FunctionCallError::RespondToModel(format!(
                        "consult responder returned invalid structured output: {error}"
                    ))
                });
            }
            EventMsg::TurnAborted(_) => {
                return Err(FunctionCallError::RespondToModel(
                    "consult responder aborted".to_string(),
                ));
            }
            EventMsg::Error(error) => {
                return Err(FunctionCallError::RespondToModel(format!(
                    "consult responder failed: {}",
                    error.message
                )));
            }
            _ => {}
        }
    }
}

fn consult_output_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["advisory", "requires_authoritative_parent"]
            },
            "advisory": { "type": "string" }
        },
        "required": ["kind", "advisory"],
        "additionalProperties": false
    })
}
