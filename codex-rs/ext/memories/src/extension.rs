use std::sync::Arc;

use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ContextualUserFragment;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::RewindContextContributionInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
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

/// Host-derived memories configuration captured for one thread.
#[derive(Clone, Debug)]
pub struct MemoriesExtensionConfig {
    /// Whether global memory context and tools are enabled.
    pub global_enabled: bool,
    /// Whether session and project scoped memory context and tools are enabled.
    pub scoped_enabled: bool,
    /// Whether the dedicated memories namespace is exposed to the model.
    pub dedicated_tools: bool,
    /// Root directory for Codex-owned memory storage.
    pub codex_home: AbsolutePathBuf,
    /// Project root used to select project-scoped memory storage.
    pub project_root: AbsolutePathBuf,
}

impl MemoriesExtensionConfig {
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

struct MemoriesConfigContributor<C> {
    config_from_host: Arc<dyn Fn(&C) -> MemoriesExtensionConfig + Send + Sync>,
}

impl<C> MemoriesConfigContributor<C> {
    fn new(
        config_from_host: impl Fn(&C) -> MemoriesExtensionConfig + Send + Sync + 'static,
    ) -> Self {
        Self {
            config_from_host: Arc::new(config_from_host),
        }
    }
}

impl ContextContributor for MemoriesExtension {
    fn contribute_thread_context_fragments<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<Box<dyn ContextualUserFragment + Send>>> {
        Box::pin(async move {
            let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
                return Vec::new();
            };
            if !config.scoped_enabled {
                return Vec::new();
            }
            config
                .backends(thread_store.level_id())
                .scoped_context_fragments()
                .await
                .into_iter()
                .map(|fragment| {
                    Box::new(fragment) as Box<dyn ContextualUserFragment + Send>
                })
                .collect()
        })
    }

    fn contribute_thread_context<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<PromptFragment>> + Send + 'a>> {
        Box::pin(async move {
            let Some(config) = thread_store.get::<MemoriesExtensionConfig>() else {
                return Vec::new();
            };
            if config.global_enabled
                && let Some(instructions) =
                    build_memory_tool_developer_instructions(&config.codex_home).await
            {
                return vec![PromptFragment::developer_policy(instructions)];
            }
            Vec::new()
        })
    }

    fn contribute_rewind_context_fragments<'a>(
        &'a self,
        input: RewindContextContributionInput<'a>,
    ) -> ExtensionFuture<'a, Vec<Box<dyn ContextualUserFragment + Send>>> {
        Box::pin(async move {
            let Some(config) = input.thread_store.get::<MemoriesExtensionConfig>() else {
                return Vec::new();
            };
            if !config.scoped_enabled {
                return Vec::new();
            }
            let Some(fragment) = config
                .backends(input.thread_store.level_id())
                .rewind_session_context_fragment(input.completed_items)
                .await
            else {
                return Vec::new();
            };
            vec![Box::new(fragment) as Box<dyn ContextualUserFragment + Send>]
        })
    }
}

impl<C> ThreadLifecycleContributor<C> for MemoriesConfigContributor<C>
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, C>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            input
                .thread_store
                .insert((self.config_from_host)(input.config));
        })
    }
}

impl<C> ConfigContributor<C> for MemoriesConfigContributor<C>
where
    C: Send + Sync + 'static,
{
    fn on_config_changed(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
        _previous_config: &C,
        new_config: &C,
    ) {
        thread_store.insert((self.config_from_host)(new_config));
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
pub fn install<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    metrics_client: Option<MetricsClient>,
    config_from_host: impl Fn(&C) -> MemoriesExtensionConfig + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    let config_contributor = Arc::new(MemoriesConfigContributor::new(config_from_host));
    let extension = Arc::new(MemoriesExtension::new(metrics_client));
    registry.thread_lifecycle_contributor(config_contributor.clone());
    registry.config_contributor(config_contributor);
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension);
}
