use std::sync::Arc;
use std::sync::Weak;

use chrono::Utc;
use codex_core::ThreadManager;
use codex_extension_api::FunctionCallError;
use codex_extension_api::JsonToolOutput;
use codex_extension_api::ResponsesApiTool;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolName;
use codex_extension_api::ToolOutput;
use codex_extension_api::ToolSpec;
use codex_protocol::ThreadId;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::AgentMailboxReadRequest;
use codex_state::StateRuntime;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::default_namespace_description;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

use crate::AGENT_MAILBOX_NAMESPACE;
use crate::READ_TOOL_NAME;
use crate::SEND_TOOL_NAME;
use crate::extension::AgentMailboxRuntime;
use crate::extension::AgentMailboxStatusNotifier;
use crate::output::AgentMailboxReadOutput;
use crate::output::snapshot_json;
use crate::output::validate_message_input_for_read_output;
use crate::schema::input_schema_for;
use crate::MAX_AGENT_MAILBOX_READ_MESSAGES;

const DEFAULT_READ_LIMIT: usize = 1;

#[derive(Clone, Copy)]
enum AgentMailboxToolKind {
    Send,
    Read,
}

pub(crate) struct AgentMailboxTool {
    kind: AgentMailboxToolKind,
    runtime: Arc<AgentMailboxRuntime>,
    state: Arc<StateRuntime>,
    thread_manager: Weak<ThreadManager>,
    notifier: Arc<dyn AgentMailboxStatusNotifier>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    target: String,
    message: String,
    category: AgentMailboxCategoryArg,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    limit: Option<usize>,
    category: Option<AgentMailboxCategoryArg>,
    sender: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(rename_all = "camelCase")]
enum AgentMailboxCategoryArg {
    Progress,
    Result,
    ActionRequired,
}

impl From<AgentMailboxCategoryArg> for AgentMailboxCategory {
    fn from(category: AgentMailboxCategoryArg) -> Self {
        match category {
            AgentMailboxCategoryArg::Progress => Self::Progress,
            AgentMailboxCategoryArg::Result => Self::Result,
            AgentMailboxCategoryArg::ActionRequired => Self::ActionRequired,
        }
    }
}

impl AgentMailboxTool {
    pub(crate) fn send(
        runtime: Arc<AgentMailboxRuntime>,
        state: Arc<StateRuntime>,
        thread_manager: Weak<ThreadManager>,
        notifier: Arc<dyn AgentMailboxStatusNotifier>,
    ) -> Self {
        Self {
            kind: AgentMailboxToolKind::Send,
            runtime,
            state,
            thread_manager,
            notifier,
        }
    }

    pub(crate) fn read(
        runtime: Arc<AgentMailboxRuntime>,
        state: Arc<StateRuntime>,
        thread_manager: Weak<ThreadManager>,
        notifier: Arc<dyn AgentMailboxStatusNotifier>,
    ) -> Self {
        Self {
            kind: AgentMailboxToolKind::Read,
            runtime,
            state,
            thread_manager,
            notifier,
        }
    }

    async fn handle_send(
        &self,
        invocation: &ToolCall,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let args: SendArgs = parse_args(invocation)?;
        let message = args.message.trim();
        if message.is_empty() {
            return Err(FunctionCallError::RespondToModel(
                "agent mailbox message must not be empty".to_string(),
            ));
        }
        let manager = self.thread_manager.upgrade().ok_or_else(|| {
            FunctionCallError::RespondToModel("agent thread manager is unavailable".to_string())
        })?;
        let target = manager
            .resolve_agent_mailbox_target(self.runtime.thread_id, args.target.trim())
            .await
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?;
        let message_id = Uuid::new_v4().to_string();
        let input = AgentMailboxMessageInput {
            id: message_id,
            root_thread_id: self.runtime.root_thread_id,
            sender_thread_id: self.runtime.thread_id,
            sender_agent_path: self.runtime.agent_path.to_string(),
            recipient_thread_id: target.thread_id,
            recipient_agent_path: target.agent_path.to_string(),
            category: args.category.into(),
            payload: AgentMailboxPayload::Plaintext {
                content: message.to_string(),
            },
            created_at: Utc::now(),
        };
        validate_message_input_for_read_output(&input)
            .map_err(FunctionCallError::RespondToModel)?;
        let outcome = self
            .state
            .agent_mailbox()
            .enqueue(input)
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to enqueue agent mailbox message: {err}"
                ))
            })?;
        self.notifier
            .status_updated(target.thread_id, outcome.snapshot.clone());
        if let Err(err) = manager
            .notify_agent_mailbox_activity(target.thread_id)
            .await
        {
            tracing::debug!(
                "mailbox message committed but live wait notification failed for {}: {err}",
                target.thread_id
            );
        }
        Ok(Box::new(JsonToolOutput::new(json!({
            "id": outcome.message.id,
            "recipient": outcome.message.recipient_agent_path,
            "mailbox": snapshot_json(&outcome.snapshot),
        }))))
    }

    async fn handle_read(
        &self,
        invocation: &ToolCall,
    ) -> Result<Box<dyn ToolOutput>, FunctionCallError> {
        let args: ReadArgs = parse_args(invocation)?;
        let limit = read_limit(args.limit)?;
        let (sender_thread_id, sender_agent_path) =
            sender_filter(self.runtime.as_ref(), args.sender.as_deref())?;
        let outcome = self
            .state
            .agent_mailbox()
            .consume(AgentMailboxReadRequest {
                root_thread_id: self.runtime.root_thread_id,
                recipient_thread_id: self.runtime.thread_id,
                sender_thread_id,
                sender_agent_path,
                category: args.category.map(Into::into),
                limit,
            })
            .await
            .map_err(|err| {
                FunctionCallError::RespondToModel(format!(
                    "failed to read agent mailbox messages: {err}"
                ))
            })?;
        if !outcome.messages.is_empty() {
            self.notifier
                .status_updated(self.runtime.thread_id, outcome.snapshot.clone());
        }
        Ok(Box::new(AgentMailboxReadOutput::new(
            outcome.messages,
            outcome.snapshot,
        )))
    }
}

fn read_limit(requested: Option<usize>) -> Result<usize, FunctionCallError> {
    let limit = requested.unwrap_or(DEFAULT_READ_LIMIT).max(1);
    if limit > MAX_AGENT_MAILBOX_READ_MESSAGES {
        return Err(FunctionCallError::RespondToModel(format!(
            "agent mailbox read limit must be at most {MAX_AGENT_MAILBOX_READ_MESSAGES} to fit the output budget"
        )));
    }
    Ok(limit)
}

impl ToolExecutor<ToolCall> for AgentMailboxTool {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(
            AGENT_MAILBOX_NAMESPACE,
            match self.kind {
                AgentMailboxToolKind::Send => SEND_TOOL_NAME,
                AgentMailboxToolKind::Read => READ_TOOL_NAME,
            },
        )
    }

    fn spec(&self) -> ToolSpec {
        match self.kind {
            AgentMailboxToolKind::Send => mailbox_function_tool::<SendArgs>(
                SEND_TOOL_NAME,
                "Send one durable, non-interrupting message to another agent mailbox.",
            ),
            AgentMailboxToolKind::Read => mailbox_function_tool::<ReadArgs>(
                READ_TOOL_NAME,
                "Read and atomically consume the oldest matching unread mailbox messages.",
            ),
        }
    }

    fn handle(&self, invocation: ToolCall) -> codex_extension_api::ToolExecutorFuture<'_> {
        Box::pin(async move {
            match self.kind {
                AgentMailboxToolKind::Send => self.handle_send(&invocation).await,
                AgentMailboxToolKind::Read => self.handle_read(&invocation).await,
            }
        })
    }
}

fn sender_filter(
    runtime: &AgentMailboxRuntime,
    sender: Option<&str>,
) -> Result<(Option<ThreadId>, Option<String>), FunctionCallError> {
    let Some(sender) = sender.map(str::trim).filter(|sender| !sender.is_empty()) else {
        return Ok((None, None));
    };
    if let Ok(thread_id) = ThreadId::from_string(sender) {
        return Ok((Some(thread_id), None));
    }
    let path = runtime
        .agent_path
        .resolve(sender)
        .map_err(FunctionCallError::RespondToModel)?;
    Ok((None, Some(path.to_string())))
}

fn parse_args<T: for<'de> Deserialize<'de>>(
    invocation: &ToolCall,
) -> Result<T, FunctionCallError> {
    let arguments = invocation.function_arguments()?;
    let value = if arguments.trim().is_empty() {
        Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str(arguments)
            .map_err(|err| FunctionCallError::RespondToModel(err.to_string()))?
    };
    serde_json::from_value(value).map_err(|err| FunctionCallError::RespondToModel(err.to_string()))
}

fn mailbox_function_tool<I: JsonSchema>(name: &str, description: &str) -> ToolSpec {
    ToolSpec::Namespace(ResponsesApiNamespace {
        name: AGENT_MAILBOX_NAMESPACE.to_string(),
        description: default_namespace_description(AGENT_MAILBOX_NAMESPACE),
        tools: vec![ResponsesApiNamespaceTool::Function(ResponsesApiTool {
            name: name.to_string(),
            description: description.to_string(),
            strict: false,
            defer_loading: None,
            parameters: codex_extension_api::parse_tool_input_schema(&input_schema_for::<I>())
                .unwrap_or_else(|err| {
                    panic!("generated input schema for {AGENT_MAILBOX_NAMESPACE}.{name}: {err}")
                }),
            output_schema: None,
        })],
    })
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
