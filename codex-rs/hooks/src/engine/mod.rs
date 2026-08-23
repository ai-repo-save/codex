pub(crate) mod command_runner;
pub(crate) mod discovery;
pub(crate) mod dispatcher;
pub(crate) mod filter_runner;
pub(crate) mod mcp_runner;
pub(crate) mod output_parser;
pub(crate) mod prompt_runner;
pub(crate) mod schema_loader;

use crate::events::compact::PostCompactRequest;
use crate::events::compact::PreCompactOutcome;
use crate::events::compact::PreCompactRequest;
use crate::events::compact::StatelessHookOutcome;
use crate::events::permission_request::PermissionRequestOutcome;
use crate::events::permission_request::PermissionRequestRequest;
use crate::events::post_tool_use::PostToolUseOutcome;
use crate::events::post_tool_use::PostToolUseRequest;
use crate::events::pre_tool_use::PreToolUseOutcome;
use crate::events::pre_tool_use::PreToolUseRequest;
use crate::events::session_end::SessionEndOutcome;
use crate::events::session_end::SessionEndRequest;
use crate::events::session_start::SessionStartOutcome;
use crate::events::session_start::SessionStartRequest;
use crate::events::stop::StopOutcome;
use crate::events::stop::StopRequest;
use crate::events::user_prompt_submit::UserPromptSubmitOutcome;
use crate::events::user_prompt_submit::UserPromptSubmitRequest;
use crate::mcp::HookMcpExecutor;
use crate::output_spill::AdditionalContextLimit;
use codex_config::ConfigLayerStack;
use codex_config::PromptHookFilterConfig;
use codex_plugin::PluginHookSource;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::protocol::HookEventName;
use codex_protocol::protocol::HookExecutionMode;
use codex_protocol::protocol::HookHandlerType;
use codex_protocol::protocol::HookRunSummary;
use codex_protocol::protocol::HookSource;
use codex_protocol::protocol::HookTrustStatus;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub use prompt_runner::PromptHookRequest;
pub use prompt_runner::PromptHookRunner;

use command_runner::CommandHookRuntime;

#[derive(Debug, Clone)]
pub(crate) struct CommandShell {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredHandler {
    pub event_name: codex_protocol::protocol::HookEventName,
    pub matcher: Option<String>,
    pub timeout_sec: u64,
    pub status_message: Option<String>,
    pub additional_context_limit: AdditionalContextLimit,
    pub source_path: AbsolutePathBuf,
    pub source: HookSource,
    pub display_order: i64,
    pub kind: ConfiguredHandlerKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfiguredHandlerKind {
    Command {
        command: String,
        env: HashMap<String, String>,
        r#async: bool,
    },
    McpTool {
        server: String,
        tool: String,
        input: serde_json::Map<String, serde_json::Value>,
    },
    Prompt {
        prompt: String,
        filter: Option<ConfiguredPromptFilter>,
        model: Option<String>,
        reasoning_effort: Option<ReasoningEffort>,
        fail_closed: bool,
        env: HashMap<String, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredPromptFilter {
    pub command: String,
    pub timeout_sec: u64,
}

#[derive(Debug)]
pub(crate) struct HandlerRunResult {
    pub started_at: i64,
    pub completed_at: i64,
    pub duration_ms: i64,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error: Option<String>,
    pub prompt_filter_skipped: bool,
}

impl HandlerRunResult {
    pub(crate) fn is_prompt_filter_skipped(&self) -> bool {
        self.prompt_filter_skipped
    }
}

impl ConfiguredHandler {
    pub(crate) fn execution_mode(&self) -> HookExecutionMode {
        match self.kind {
            ConfiguredHandlerKind::Command { r#async: true, .. } => HookExecutionMode::Async,
            ConfiguredHandlerKind::Command { r#async: false, .. }
            | ConfiguredHandlerKind::McpTool { .. }
            | ConfiguredHandlerKind::Prompt { .. } => HookExecutionMode::Sync,
        }
    }

    /// Only synchronous hooks can apply control effects.
    pub(crate) fn can_apply_control_effects(&self) -> bool {
        self.execution_mode() == HookExecutionMode::Sync
    }

    pub fn run_id(&self) -> String {
        format!(
            "{}:{}:{}",
            self.event_name_label(),
            self.display_order,
            self.source_path.display()
        )
    }

    fn event_name_label(&self) -> &'static str {
        match self.event_name {
            codex_protocol::protocol::HookEventName::PreToolUse => "pre-tool-use",
            codex_protocol::protocol::HookEventName::PermissionRequest => "permission-request",
            codex_protocol::protocol::HookEventName::PostToolUse => "post-tool-use",
            codex_protocol::protocol::HookEventName::PreCompact => "pre-compact",
            codex_protocol::protocol::HookEventName::PostCompact => "post-compact",
            codex_protocol::protocol::HookEventName::SessionStart => "session-start",
            codex_protocol::protocol::HookEventName::SessionEnd => "session-end",
            codex_protocol::protocol::HookEventName::UserPromptSubmit => "user-prompt-submit",
            codex_protocol::protocol::HookEventName::SubagentStart => "subagent-start",
            codex_protocol::protocol::HookEventName::SubagentStop => "subagent-stop",
            codex_protocol::protocol::HookEventName::Stop => "stop",
        }
    }

    pub(crate) fn handler_type(&self) -> HookHandlerType {
        match &self.kind {
            ConfiguredHandlerKind::Command { .. } => HookHandlerType::Command,
            ConfiguredHandlerKind::McpTool { .. } => HookHandlerType::McpTool,
            ConfiguredHandlerKind::Prompt { .. } => HookHandlerType::Prompt,
        }
    }

    pub(crate) fn fail_closed(&self) -> bool {
        match &self.kind {
            ConfiguredHandlerKind::Prompt { fail_closed, .. } => *fail_closed,
            ConfiguredHandlerKind::Command { .. } | ConfiguredHandlerKind::McpTool { .. } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookListEntryHandler {
    Command { command: String, r#async: bool },
    McpTool { server: String, tool: String },
    Prompt,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookListEntry {
    pub key: String,
    pub id: Option<String>,
    pub event_name: HookEventName,
    pub handler: HookListEntryHandler,
    pub matcher: Option<String>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub filter: Option<PromptHookFilterConfig>,
    pub fail_closed: Option<bool>,
    pub timeout_sec: u64,
    pub status_message: Option<String>,
    pub additional_context_limit: Option<usize>,
    pub source_path: AbsolutePathBuf,
    pub source: HookSource,
    pub plugin_id: Option<String>,
    pub display_order: i64,
    pub enabled: bool,
    pub is_managed: bool,
    pub current_hash: String,
    pub trust_status: HookTrustStatus,
}

#[derive(Clone)]
pub(crate) struct ClaudeHooksEngine {
    pub(crate) handlers: Vec<ConfiguredHandler>,
    warnings: Vec<String>,
    required_load_errors: Vec<String>,
    pub(crate) command_runtime: CommandHookRuntime,
    pub(crate) mcp_executor: Arc<dyn HookMcpExecutor>,
    pub(crate) prompt_hook_runner: Option<Arc<dyn PromptHookRunner>>,
}

impl ClaudeHooksEngine {
    #[cfg(test)]
    pub(crate) fn new(
        enabled: bool,
        bypass_hook_trust: bool,
        config_layer_stack: Option<&ConfigLayerStack>,
        plugin_hook_sources: Vec<PluginHookSource>,
        plugin_hook_load_warnings: Vec<String>,
        command_runtime: CommandHookRuntime,
        mcp_executor: Arc<dyn HookMcpExecutor>,
    ) -> Self {
        Self::new_with_prompt_runner(
            enabled,
            bypass_hook_trust,
            config_layer_stack,
            plugin_hook_sources,
            plugin_hook_load_warnings,
            command_runtime,
            mcp_executor,
            /*prompt_hook_runner*/ None,
        )
    }

    pub(crate) fn new_with_prompt_runner(
        enabled: bool,
        bypass_hook_trust: bool,
        config_layer_stack: Option<&ConfigLayerStack>,
        plugin_hook_sources: Vec<PluginHookSource>,
        plugin_hook_load_warnings: Vec<String>,
        command_runtime: CommandHookRuntime,
        mcp_executor: Arc<dyn HookMcpExecutor>,
        prompt_hook_runner: Option<Arc<dyn PromptHookRunner>>,
    ) -> Self {
        if !enabled {
            return Self {
                handlers: Vec::new(),
                warnings: Vec::new(),
                required_load_errors: Vec::new(),
                command_runtime,
                mcp_executor,
                prompt_hook_runner,
            };
        }

        let _ = schema_loader::generated_hook_schemas();
        let discovered = discovery::discover_handlers(
            config_layer_stack,
            plugin_hook_sources,
            plugin_hook_load_warnings,
            bypass_hook_trust,
        );
        Self {
            handlers: discovered.handlers,
            warnings: discovered.warnings,
            required_load_errors: discovered.required_load_errors,
            command_runtime,
            mcp_executor,
            prompt_hook_runner,
        }
    }

    pub(crate) fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) fn required_load_errors(&self) -> &[String] {
        &self.required_load_errors
    }

    pub(crate) fn preview_session_start(
        &self,
        request: &SessionStartRequest,
    ) -> Vec<HookRunSummary> {
        crate::events::session_start::preview(&self.handlers, request)
    }

    pub(crate) fn preview_pre_tool_use(&self, request: &PreToolUseRequest) -> Vec<HookRunSummary> {
        crate::events::pre_tool_use::preview(&self.handlers, request)
    }

    pub(crate) fn preview_permission_request(
        &self,
        request: &PermissionRequestRequest,
    ) -> Vec<HookRunSummary> {
        crate::events::permission_request::preview(&self.handlers, request)
    }

    pub(crate) fn max_permission_request_timeout(&self) -> Duration {
        Duration::from_secs(
            self.handlers
                .iter()
                .filter(|handler| {
                    handler.event_name == HookEventName::PermissionRequest
                        && handler.can_apply_control_effects()
                })
                .map(|handler| {
                    let filter_timeout = match &handler.kind {
                        ConfiguredHandlerKind::Prompt {
                            filter: Some(filter),
                            ..
                        } => filter.timeout_sec,
                        ConfiguredHandlerKind::Command { .. }
                        | ConfiguredHandlerKind::McpTool { .. }
                        | ConfiguredHandlerKind::Prompt { filter: None, .. } => 0,
                    };
                    handler.timeout_sec.saturating_add(filter_timeout)
                })
                .max()
                .unwrap_or_default(),
        )
    }

    pub(crate) fn preview_post_tool_use(
        &self,
        request: &PostToolUseRequest,
    ) -> Vec<HookRunSummary> {
        crate::events::post_tool_use::preview(&self.handlers, request)
    }

    pub(crate) async fn run_session_start(
        &self,
        request: SessionStartRequest,
        turn_id: Option<String>,
    ) -> SessionStartOutcome {
        crate::events::session_start::run(self, request, turn_id).await
    }

    pub(crate) async fn run_pre_tool_use(&self, request: PreToolUseRequest) -> PreToolUseOutcome {
        crate::events::pre_tool_use::run(self, request).await
    }

    pub(crate) async fn run_permission_request(
        &self,
        request: PermissionRequestRequest,
    ) -> PermissionRequestOutcome {
        crate::events::permission_request::run(self, request).await
    }

    pub(crate) async fn run_post_tool_use(
        &self,
        request: PostToolUseRequest,
    ) -> PostToolUseOutcome {
        let mut outcome = crate::events::post_tool_use::run(self, request).await;
        if let Some(feedback_message) = outcome.feedback_message.take() {
            outcome.feedback_message = Some(
                self.command_runtime
                    .output_spiller()
                    .maybe_spill_text(feedback_message)
                    .await,
            );
        }
        if let Some(updated_tool_output) = outcome.updated_tool_output.take() {
            outcome.updated_tool_output = Some(
                self.command_runtime
                    .output_spiller()
                    .maybe_spill_text(updated_tool_output)
                    .await,
            );
        }
        outcome
    }

    pub(crate) fn preview_pre_compact(&self, request: &PreCompactRequest) -> Vec<HookRunSummary> {
        crate::events::compact::preview_pre(&self.handlers, request)
    }

    pub(crate) async fn run_pre_compact(&self, request: PreCompactRequest) -> PreCompactOutcome {
        crate::events::compact::run_pre(self, request).await
    }

    pub(crate) fn preview_post_compact(&self, request: &PostCompactRequest) -> Vec<HookRunSummary> {
        crate::events::compact::preview_post(&self.handlers, request)
    }

    pub(crate) async fn run_post_compact(
        &self,
        request: PostCompactRequest,
    ) -> StatelessHookOutcome {
        let mut outcome = crate::events::compact::run_post(self, request).await;
        if let Some(supplement) = outcome.supplement.take() {
            outcome.supplement = Some(
                self.command_runtime
                    .output_spiller()
                    .maybe_spill_text(supplement)
                    .await,
            );
        }
        outcome
    }

    pub(crate) fn preview_user_prompt_submit(
        &self,
        request: &UserPromptSubmitRequest,
    ) -> Vec<HookRunSummary> {
        crate::events::user_prompt_submit::preview(&self.handlers, request)
    }

    pub(crate) async fn run_user_prompt_submit(
        &self,
        request: UserPromptSubmitRequest,
    ) -> UserPromptSubmitOutcome {
        crate::events::user_prompt_submit::run(self, request).await
    }

    pub(crate) fn preview_stop(&self, request: &StopRequest) -> Vec<HookRunSummary> {
        crate::events::stop::preview(&self.handlers, request)
    }

    pub(crate) fn preview_session_end(&self) -> Vec<HookRunSummary> {
        crate::events::session_end::preview(&self.handlers)
    }

    pub(crate) async fn run_session_end(&self, request: SessionEndRequest) -> SessionEndOutcome {
        crate::events::session_end::run(self, request).await
    }

    pub(crate) async fn run_stop(&self, request: StopRequest) -> StopOutcome {
        let mut outcome = crate::events::stop::run(self, request).await;
        outcome.continuation_fragments = self
            .command_runtime
            .output_spiller()
            .maybe_spill_prompt_fragments(outcome.continuation_fragments)
            .await;
        outcome
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
