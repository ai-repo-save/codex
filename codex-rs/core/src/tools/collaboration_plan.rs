use crate::agent::exceeds_thread_spawn_depth_limit;
use crate::agent::next_thread_spawn_depth;
use crate::session::turn_context::TurnContext;
use crate::tools::context::ToolInvocation;
use crate::tools::handlers::multi_agents::CloseAgentHandler;
use crate::tools::handlers::multi_agents::ResumeAgentHandler;
use crate::tools::handlers::multi_agents::SendInputHandler;
use crate::tools::handlers::multi_agents::SpawnAgentHandler;
use crate::tools::handlers::multi_agents::WaitAgentHandler;
use crate::tools::handlers::multi_agents_common::DEFAULT_WAIT_TIMEOUT_MS;
use crate::tools::handlers::multi_agents_common::MAX_WAIT_TIMEOUT_MS;
use crate::tools::handlers::multi_agents_common::MIN_WAIT_TIMEOUT_MS;
use crate::tools::handlers::multi_agents_v2::AskParentHandler;
use crate::tools::handlers::multi_agents_v2::FollowupTaskHandler as FollowupTaskHandlerV2;
use crate::tools::handlers::multi_agents_v2::InspectAgentHandlerV2;
use crate::tools::handlers::multi_agents_v2::InterruptAgentHandler;
use crate::tools::handlers::multi_agents_v2::ListAgentsHandler as ListAgentsHandlerV2;
use crate::tools::handlers::multi_agents_v2::SendMessageHandler as SendMessageHandlerV2;
use crate::tools::handlers::multi_agents_v2::SpawnAgentHandler as SpawnAgentHandlerV2;
use crate::tools::handlers::multi_agents_v2::WaitAgentHandler as WaitAgentHandlerV2;
use crate::tools::registry::CoreToolRuntime;
use crate::tools::registry::ToolExposure;
use crate::tools::registry::override_tool_exposure;
use codex_agent_control::SpawnAgentToolOptions;
use codex_agent_control::WaitAgentTimeoutOptions;
use codex_protocol::protocol::MultiAgentVersion;
use codex_tools::ResponsesApiNamespace;
use codex_tools::ResponsesApiNamespaceTool;
use codex_tools::ToolExecutor;
use codex_tools::ToolName;
use codex_tools::ToolSearchInfo;
use codex_tools::ToolSpec;
use std::sync::Arc;

const MULTI_AGENT_V2_NAMESPACE_DESCRIPTION: &str = "Tools for spawning and managing sub-agents.";

pub(super) fn build(turn_context: &TurnContext) -> Vec<Arc<dyn CoreToolRuntime>> {
    if !collaboration_tools_enabled(turn_context) {
        return Vec::new();
    }

    if multi_agent_v2_enabled(turn_context) {
        build_v2(turn_context)
    } else {
        build_v1(turn_context)
    }
}

fn build_v1(turn_context: &TurnContext) -> Vec<Arc<dyn CoreToolRuntime>> {
    let agent_type_description = agent_type_description(turn_context);
    let exposure = if search_tool_enabled(turn_context) {
        ToolExposure::Deferred
    } else {
        ToolExposure::Direct
    };
    let wait_agent_timeouts = wait_agent_timeout_options(turn_context);
    vec![
        override_tool_exposure(
            Arc::new(SpawnAgentHandler::new(SpawnAgentToolOptions {
                available_models: turn_context.available_models.clone(),
                agent_type_description,
                expose_agent_type: !turn_context.config.agent_roles.is_empty(),
                hide_agent_type_model_reasoning: false,
                expose_spawn_agent_model_overrides: true,
                usage_hint_text: turn_context.config.multi_agent_v2.usage_hint_text.clone(),
                max_concurrent_threads_per_session: Some(
                    turn_context
                        .config
                        .multi_agent_v2
                        .max_concurrent_threads_per_session,
                ),
                encrypt_messages: false,
            })),
            exposure,
        ),
        override_tool_exposure(Arc::new(SendInputHandler), exposure),
        override_tool_exposure(Arc::new(ResumeAgentHandler), exposure),
        override_tool_exposure(
            Arc::new(WaitAgentHandler::new(wait_agent_timeouts)),
            exposure,
        ),
        override_tool_exposure(Arc::new(CloseAgentHandler), exposure),
    ]
}

fn build_v2(turn_context: &TurnContext) -> Vec<Arc<dyn CoreToolRuntime>> {
    let exposure = if turn_context.config.multi_agent_v2.non_code_mode_only {
        ToolExposure::DirectModelOnly
    } else {
        ToolExposure::Direct
    };
    let tool_namespace = namespace_tools_enabled(turn_context)
        .then_some(turn_context.config.multi_agent_v2.tool_namespace.as_deref())
        .flatten();
    let hide_spawn_agent_metadata = turn_context.config.multi_agent_v2.hide_spawn_agent_metadata;
    let mut runtimes = vec![
        override_tool_exposure(
            multi_agent_v2_handler(
                SpawnAgentHandlerV2::new(SpawnAgentToolOptions {
                    available_models: turn_context.available_models.clone(),
                    agent_type_description: agent_type_description(turn_context),
                    expose_agent_type: !turn_context.config.agent_roles.is_empty(),
                    hide_agent_type_model_reasoning: hide_spawn_agent_metadata,
                    expose_spawn_agent_model_overrides: turn_context
                        .config
                        .multi_agent_v2
                        .expose_spawn_agent_model_overrides,
                    usage_hint_text: turn_context.config.multi_agent_v2.usage_hint_text.clone(),
                    max_concurrent_threads_per_session: Some(
                        turn_context
                            .config
                            .multi_agent_v2
                            .max_concurrent_threads_per_session,
                    ),
                    encrypt_messages: turn_context.config.multi_agent_v2.encrypt_messages,
                }),
                tool_namespace,
            ),
            exposure,
        ),
        override_tool_exposure(
            multi_agent_v2_handler(
                SendMessageHandlerV2::new(turn_context.config.multi_agent_v2.encrypt_messages),
                tool_namespace,
            ),
            exposure,
        ),
        override_tool_exposure(
            multi_agent_v2_handler(
                FollowupTaskHandlerV2::new(turn_context.config.multi_agent_v2.encrypt_messages),
                tool_namespace,
            ),
            exposure,
        ),
    ];
    if turn_context.parent_thread_id.is_some() {
        runtimes.push(override_tool_exposure(
            multi_agent_v2_handler(AskParentHandler, tool_namespace),
            exposure,
        ));
    }
    runtimes.extend([
        override_tool_exposure(
            multi_agent_v2_handler(
                WaitAgentHandlerV2::new(wait_agent_timeout_options(turn_context)),
                tool_namespace,
            ),
            exposure,
        ),
        override_tool_exposure(
            multi_agent_v2_handler(InterruptAgentHandler, tool_namespace),
            exposure,
        ),
        override_tool_exposure(
            multi_agent_v2_handler(ListAgentsHandlerV2, tool_namespace),
            exposure,
        ),
        override_tool_exposure(
            multi_agent_v2_handler(InspectAgentHandlerV2, tool_namespace),
            exposure,
        ),
    ]);
    runtimes
}

fn collaboration_tools_enabled(turn_context: &TurnContext) -> bool {
    match turn_context.multi_agent_version {
        MultiAgentVersion::Disabled => false,
        MultiAgentVersion::V1 => !exceeds_thread_spawn_depth_limit(
            next_thread_spawn_depth(&turn_context.session_source),
            turn_context.config.agent_max_depth,
        ),
        MultiAgentVersion::V2 => true,
    }
}

fn multi_agent_v2_enabled(turn_context: &TurnContext) -> bool {
    turn_context.multi_agent_version == MultiAgentVersion::V2
}

fn namespace_tools_enabled(turn_context: &TurnContext) -> bool {
    turn_context.provider.capabilities().namespace_tools
}

fn search_tool_enabled(turn_context: &TurnContext) -> bool {
    turn_context.model_info.supports_search_tool && namespace_tools_enabled(turn_context)
}

fn wait_agent_timeout_options(turn_context: &TurnContext) -> WaitAgentTimeoutOptions {
    if multi_agent_v2_enabled(turn_context) {
        return WaitAgentTimeoutOptions {
            default_timeout_ms: turn_context.config.multi_agent_v2.default_wait_timeout_ms,
            min_timeout_ms: turn_context.config.multi_agent_v2.min_wait_timeout_ms,
            max_timeout_ms: turn_context.config.multi_agent_v2.max_wait_timeout_ms,
        };
    }

    WaitAgentTimeoutOptions {
        default_timeout_ms: DEFAULT_WAIT_TIMEOUT_MS,
        min_timeout_ms: MIN_WAIT_TIMEOUT_MS,
        max_timeout_ms: MAX_WAIT_TIMEOUT_MS,
    }
}

fn agent_type_description(turn_context: &TurnContext) -> String {
    let agent_type_description =
        crate::agent::role::spawn_tool_spec::build(&turn_context.config.agent_roles);
    if agent_type_description.is_empty() {
        crate::agent::role::spawn_tool_spec::build(&std::collections::BTreeMap::new())
    } else {
        agent_type_description
    }
}

fn multi_agent_v2_handler(
    handler: impl CoreToolRuntime + 'static,
    namespace: Option<&str>,
) -> Arc<dyn CoreToolRuntime> {
    match namespace {
        Some(namespace) => Arc::new(MultiAgentV2NamespaceOverride {
            handler: Arc::new(handler),
            namespace: namespace.to_string(),
        }),
        None => Arc::new(handler),
    }
}

struct MultiAgentV2NamespaceOverride {
    handler: Arc<dyn CoreToolRuntime>,
    namespace: String,
}

impl ToolExecutor<ToolInvocation> for MultiAgentV2NamespaceOverride {
    fn tool_name(&self) -> ToolName {
        ToolName::namespaced(self.namespace.clone(), self.handler.tool_name().name)
    }

    fn spec(&self) -> ToolSpec {
        match self.handler.spec() {
            ToolSpec::Function(tool) => ToolSpec::Namespace(ResponsesApiNamespace {
                name: self.namespace.clone(),
                description: MULTI_AGENT_V2_NAMESPACE_DESCRIPTION.to_string(),
                tools: vec![ResponsesApiNamespaceTool::Function(tool)],
            }),
            spec => spec,
        }
    }

    fn exposure(&self) -> ToolExposure {
        self.handler.exposure()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.handler.supports_parallel_tool_calls()
    }

    fn search_info(&self) -> Option<ToolSearchInfo> {
        self.handler.search_info()
    }

    fn handle(&self, invocation: ToolInvocation) -> codex_tools::ToolExecutorFuture<'_> {
        self.handler.handle(invocation)
    }
}

impl CoreToolRuntime for MultiAgentV2NamespaceOverride {
    fn matches_kind(&self, payload: &crate::tools::context::ToolPayload) -> bool {
        self.handler.matches_kind(payload)
    }

    fn create_diff_consumer(
        &self,
    ) -> Option<Box<dyn crate::tools::registry::ToolArgumentDiffConsumer>> {
        self.handler.create_diff_consumer()
    }
}
