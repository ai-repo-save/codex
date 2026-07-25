use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use chrono::Utc;
use codex_core::ThreadManager;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::TerminalMessageContributor;
use codex_extension_api::TerminalMessageDisposition;
use codex_extension_api::TerminalMessageInput;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolContributor;
use codex_extension_api::WorldStateContributionInput;
use codex_extension_api::WorldStateSectionContribution;
use codex_protocol::AgentPath;
use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxMessageInput;
use codex_state::AgentMailboxPayload;
use codex_state::AgentMailboxUnreadSnapshot;
use codex_state::StateRuntime;
use uuid::Uuid;

use crate::output::validate_payload;
use crate::tools::AgentMailboxTool;
use crate::world_state::mailbox_world_state_section;

pub trait AgentMailboxStatusNotifier: Send + Sync {
    fn status_updated(&self, thread_id: ThreadId, snapshot: AgentMailboxUnreadSnapshot);
}

pub struct NoopAgentMailboxStatusNotifier;

impl AgentMailboxStatusNotifier for NoopAgentMailboxStatusNotifier {
    fn status_updated(&self, _thread_id: ThreadId, _snapshot: AgentMailboxUnreadSnapshot) {}
}

/// Host-derived mailbox configuration captured for one thread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentMailboxExtensionConfig {
    /// Whether mailbox tools and contributions are enabled for the thread.
    pub enabled: bool,
}

pub(crate) struct AgentMailboxRuntime {
    pub(crate) thread_id: ThreadId,
    pub(crate) root_thread_id: ThreadId,
    pub(crate) agent_path: AgentPath,
    pub(crate) persistent_thread_state_available: bool,
    pub(crate) enabled: AtomicBool,
}

impl AgentMailboxRuntime {
    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(
            self.persistent_thread_state_available && enabled,
            Ordering::Release,
        );
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}

struct AgentMailboxConfigContributor<C> {
    config_from_host: Arc<dyn Fn(&C) -> AgentMailboxExtensionConfig + Send + Sync>,
}

impl<C> AgentMailboxConfigContributor<C> {
    fn new(
        config_from_host: impl Fn(&C) -> AgentMailboxExtensionConfig + Send + Sync + 'static,
    ) -> Self {
        Self {
            config_from_host: Arc::new(config_from_host),
        }
    }
}

pub struct AgentMailboxExtension {
    state: Arc<StateRuntime>,
    thread_manager: Weak<ThreadManager>,
    notifier: Arc<dyn AgentMailboxStatusNotifier>,
}

impl AgentMailboxExtension {
    fn new(
        state: Arc<StateRuntime>,
        thread_manager: Weak<ThreadManager>,
        notifier: Arc<dyn AgentMailboxStatusNotifier>,
    ) -> Self {
        Self {
            state,
            thread_manager,
            notifier,
        }
    }
}

impl<C> ThreadLifecycleContributor<C> for AgentMailboxConfigContributor<C>
where
    C: Send + Sync + 'static,
{
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, C>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Ok(thread_id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            let Ok(session_id) = SessionId::try_from(input.session_store.level_id()) else {
                return;
            };
            let root_thread_id: ThreadId = session_id.into();
            let agent_path = input
                .session_source
                .get_agent_path()
                .unwrap_or_else(AgentPath::root);
            let enabled = input.persistent_thread_state_available
                && (self.config_from_host)(input.config).enabled;
            input
                .thread_store
                .get_or_init::<AgentMailboxRuntime>(|| AgentMailboxRuntime {
                    thread_id,
                    root_thread_id,
                    agent_path,
                    persistent_thread_state_available: input.persistent_thread_state_available,
                    enabled: AtomicBool::new(enabled),
                })
                .set_enabled(enabled);
        })
    }
}

impl<C> ConfigContributor<C> for AgentMailboxConfigContributor<C>
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
        if let Some(runtime) = thread_store.get::<AgentMailboxRuntime>() {
            runtime.set_enabled((self.config_from_host)(new_config).enabled);
        }
    }
}

impl ToolContributor for AgentMailboxExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn codex_extension_api::ToolExecutor<codex_extension_api::ToolCall>>> {
        let Some(runtime) = thread_store.get::<AgentMailboxRuntime>() else {
            return Vec::new();
        };
        if !runtime.enabled() {
            return Vec::new();
        }
        vec![
            Arc::new(AgentMailboxTool::send(
                Arc::clone(&runtime),
                Arc::clone(&self.state),
                self.thread_manager.clone(),
                Arc::clone(&self.notifier),
            )),
            Arc::new(AgentMailboxTool::read(
                runtime,
                Arc::clone(&self.state),
                self.thread_manager.clone(),
                Arc::clone(&self.notifier),
            )),
        ]
    }
}

impl ContextContributor for AgentMailboxExtension {
    fn contribute_world_state<'a>(
        &'a self,
        input: WorldStateContributionInput<'a>,
    ) -> ExtensionFuture<'a, Vec<WorldStateSectionContribution>> {
        Box::pin(async move {
            let Some(runtime) = input.thread_store.get::<AgentMailboxRuntime>() else {
                return Vec::new();
            };
            if !runtime.enabled() {
                return Vec::new();
            }
            match self
                .state
                .agent_mailbox()
                .unread_snapshot(runtime.root_thread_id, runtime.thread_id)
                .await
            {
                Ok(snapshot) => vec![mailbox_world_state_section(snapshot)],
                Err(err) => {
                    tracing::warn!(
                        "failed to read agent mailbox World State for {}: {err}",
                        runtime.thread_id
                    );
                    Vec::new()
                }
            }
        })
    }
}

impl TerminalMessageContributor for AgentMailboxExtension {
    fn contribute<'a>(
        &'a self,
        input: TerminalMessageInput<'a>,
    ) -> ExtensionFuture<'a, Result<TerminalMessageDisposition, String>> {
        Box::pin(async move {
            let Some(runtime) = input.recipient_thread_store.get::<AgentMailboxRuntime>() else {
                return Ok(TerminalMessageDisposition::Unclaimed);
            };
            if !runtime.enabled() {
                return Ok(TerminalMessageDisposition::Unclaimed);
            }
            let category = match input.status {
                AgentStatus::Completed(_) => AgentMailboxCategory::Result,
                AgentStatus::Errored(_) | AgentStatus::Shutdown | AgentStatus::NotFound => {
                    AgentMailboxCategory::ActionRequired
                }
                AgentStatus::PendingInit | AgentStatus::Running | AgentStatus::Interrupted => {
                    return Ok(TerminalMessageDisposition::Unclaimed);
                }
            };
            let payload = match input.communication.encrypted_content.as_ref() {
                Some(encrypted_content) => AgentMailboxPayload::Encrypted {
                    encrypted_content: encrypted_content.clone(),
                },
                None => AgentMailboxPayload::Plaintext {
                    content: input.communication.content.clone(),
                },
            };
            let message_id = input
                .communication
                .id
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            let root_thread_id: ThreadId = input.session_id.into();
            let message = AgentMailboxMessageInput {
                id: message_id,
                root_thread_id,
                sender_thread_id: input.sender_thread_id,
                sender_agent_path: input.communication.author.to_string(),
                recipient_thread_id: input.recipient_thread_id,
                recipient_agent_path: input.communication.recipient.to_string(),
                category,
                payload,
                created_at: Utc::now(),
            };
            validate_payload(&message.payload).map_err(|err| {
                format!("failed to capture terminal agent mailbox message: {err}")
            })?;
            let outcome = self
                .state
                .agent_mailbox()
                .enqueue(message)
                .await
                .map_err(|err| {
                    format!("failed to capture terminal agent mailbox message: {err}")
                })?;
            self.notifier
                .status_updated(input.recipient_thread_id, outcome.snapshot);
            if let Some(manager) = self.thread_manager.upgrade()
                && let Err(err) = manager
                    .notify_agent_mailbox_activity(input.recipient_thread_id)
                    .await
            {
                tracing::debug!(
                    "terminal mailbox message committed but live wait notification failed for {}: {err}",
                    input.recipient_thread_id
                );
            }
            Ok(TerminalMessageDisposition::Committed)
        })
    }
}

pub fn install_with_backend<C>(
    registry: &mut ExtensionRegistryBuilder<C>,
    state: Arc<StateRuntime>,
    thread_manager: Weak<ThreadManager>,
    notifier: Arc<dyn AgentMailboxStatusNotifier>,
    config_from_host: impl Fn(&C) -> AgentMailboxExtensionConfig + Send + Sync + 'static,
) where
    C: Send + Sync + 'static,
{
    let config_contributor = Arc::new(AgentMailboxConfigContributor::new(config_from_host));
    let extension = Arc::new(AgentMailboxExtension::new(state, thread_manager, notifier));
    registry.thread_lifecycle_contributor(config_contributor.clone());
    registry.config_contributor(config_contributor);
    registry.prompt_contributor(extension.clone());
    registry.terminal_message_contributor(extension.clone());
    registry.tool_contributor(extension);
}

#[cfg(test)]
#[path = "extension_tests.rs"]
mod tests;
