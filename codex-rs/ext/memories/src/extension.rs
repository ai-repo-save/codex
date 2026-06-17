use std::sync::Arc;

use codex_core::config::Config;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::PromptSlot;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
use codex_features::Feature;
use codex_otel::MetricsClient;
use codex_utils_absolute_path::AbsolutePathBuf;

use crate::prompts::build_memory_tool_developer_instructions;
use crate::scoped::MemoryToolBackends;
use crate::tools;

/// Contributes Codex memory read-path prompt context and memory read tools.
#[derive(Clone, Default)]
pub(crate) struct MemoriesExtension {
    metrics_client: Option<MetricsClient>,
}

impl MemoriesExtension {
    fn new(metrics_client: Option<MetricsClient>) -> Self {
        Self { metrics_client }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MemoriesExtensionConfig {
    pub(crate) global_enabled: bool,
    pub(crate) scoped_enabled: bool,
    pub(crate) dedicated_tools: bool,
    pub(crate) codex_home: AbsolutePathBuf,
    pub(crate) project_root: AbsolutePathBuf,
}

impl MemoriesExtensionConfig {
    fn from_config(config: &Config) -> Self {
        Self {
            global_enabled: config.features.enabled(Feature::MemoryTool)
                && config.memories.use_memories,
            scoped_enabled: config.features.enabled(Feature::MemoryTool)
                && config.memories.use_scoped_memories,
            dedicated_tools: config.memories.dedicated_tools,
            codex_home: config.codex_home.clone(),
            project_root: config.project_root.clone(),
        }
    }

    fn backends(&self, thread_id: &str) -> MemoryToolBackends {
        MemoryToolBackends::new(
            &self.codex_home,
            self.global_enabled,
            self.scoped_enabled,
            thread_id,
            &self.project_root,
        )
    }
}

impl ContextContributor for MemoriesExtension {
    fn contribute<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<PromptFragment>> + Send + 'a>> {
        Box::pin(async move {
            let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
                return Vec::new();
            };
            let mut fragments = Vec::new();
            if config.global_enabled
                && let Some(instructions) =
                    build_memory_tool_developer_instructions(&config.codex_home).await
            {
                fragments.push(PromptFragment::developer_policy(instructions));
            }
            if config.scoped_enabled
                && let Some(context) = config
                    .backends(thread_store.level_id())
                    .scoped_context_fragment()
                    .await
            {
                fragments.push(PromptFragment::new(PromptSlot::ContextualUser, context));
            }
            fragments
        })
    }
}

impl ThreadLifecycleContributor<Config> for MemoriesExtension {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert(MemoriesExtensionConfig::from_config(input.config));
        })
    }
}

impl ConfigContributor<Config> for MemoriesExtension {
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &Config,
        new_config: &Config,
    ) {
        thread_store.insert(MemoriesExtensionConfig::from_config(new_config));
    }
}

impl ToolContributor for MemoriesExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn codex_extension_api::ToolExecutor<codex_extension_api::ToolCall>>> {
        let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
            return Vec::new();
        };
        if (!config.global_enabled && !config.scoped_enabled) || !config.dedicated_tools {
            return Vec::new();
        }

        tools::memory_tools(
            config.backends(thread_store.level_id()),
            self.metrics_client.clone(),
        )
    }
}

/// Installs the memories extension contributors into the extension registry.
pub fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    metrics_client: Option<MetricsClient>,
) {
    let extension = Arc::new(MemoriesExtension::new(metrics_client));
    registry.thread_lifecycle_contributor(extension.clone());
    registry.config_contributor(extension.clone());
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension);
}
