use std::sync::Arc;

use codex_app_server_protocol::AgentMailboxStatus;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::ThreadAgentMailboxGetParams;
use codex_app_server_protocol::ThreadAgentMailboxGetResponse;
use codex_app_server_protocol::ThreadAgentMailboxUpdatedNotification;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_features::Feature;
use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::RolloutItem;
use codex_rollout::state_db::StateDbHandle;
use codex_state::AgentMailboxUnreadSnapshot;
use codex_thread_store::ReadThreadParams;
use codex_thread_store::ThreadStore;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::OutgoingMessageSender;

#[derive(Clone)]
pub(crate) struct AgentMailboxRequestProcessor {
    thread_manager: Arc<ThreadManager>,
    outgoing: Arc<OutgoingMessageSender>,
    config: Arc<Config>,
    thread_store: Arc<dyn ThreadStore>,
    state_db: Option<StateDbHandle>,
}

impl AgentMailboxRequestProcessor {
    pub(crate) fn new(
        thread_manager: Arc<ThreadManager>,
        outgoing: Arc<OutgoingMessageSender>,
        config: Arc<Config>,
        thread_store: Arc<dyn ThreadStore>,
        state_db: Option<StateDbHandle>,
    ) -> Self {
        Self {
            thread_manager,
            outgoing,
            config,
            thread_store,
            state_db,
        }
    }

    pub(crate) async fn thread_agent_mailbox_get(
        &self,
        params: ThreadAgentMailboxGetParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let thread_id = parse_thread_id_for_request(&params.thread_id)?;
        let mailbox = self.unread_status(thread_id).await?;
        Ok(Some(ThreadAgentMailboxGetResponse { mailbox }.into()))
    }

    pub(crate) async fn emit_snapshot_to_connection(
        &self,
        thread_id: ThreadId,
        connection_id: ConnectionId,
    ) {
        let mailbox = match self.unread_status(thread_id).await {
            Ok(mailbox) => mailbox,
            Err(error) => {
                tracing::warn!(
                    "failed to read agent mailbox status while hydrating {thread_id}: {}",
                    error.message
                );
                return;
            }
        };
        self.outgoing
            .send_server_notification_to_connections(
                &[connection_id],
                ServerNotification::ThreadAgentMailboxUpdated(
                    ThreadAgentMailboxUpdatedNotification {
                        thread_id: thread_id.to_string(),
                        mailbox,
                    },
                ),
            )
            .await;
    }

    async fn unread_status(
        &self,
        thread_id: ThreadId,
    ) -> Result<AgentMailboxStatus, JSONRPCErrorError> {
        if !self.config.features.enabled(Feature::AgentMailbox) {
            return Ok(empty_agent_mailbox_status());
        }
        let Some(state_db) = self.state_db.as_ref() else {
            return Ok(empty_agent_mailbox_status());
        };
        let root_thread_id = self.root_thread_id(thread_id).await?;
        let snapshot = state_db
            .agent_mailbox()
            .unread_snapshot(root_thread_id, thread_id)
            .await
            .map_err(|error| {
                internal_error(format!("failed to read agent mailbox status for {thread_id}: {error}"))
            })?;
        Ok(agent_mailbox_status(snapshot))
    }

    async fn root_thread_id(&self, thread_id: ThreadId) -> Result<ThreadId, JSONRPCErrorError> {
        if let Ok(thread) = self.thread_manager.get_thread(thread_id).await {
            let root_thread_id: ThreadId = thread.session_configured().session_id.into();
            return Ok(root_thread_id);
        }

        let stored_thread = self
            .thread_store
            .read_thread(ReadThreadParams {
                thread_id,
                include_archived: true,
                include_history: true,
            })
            .await
            .map_err(|error| invalid_request(format!("thread not found: {thread_id}: {error}")))?;
        if let Some(session_id) = stored_thread.history.as_ref().and_then(session_id_from_history) {
            let root_thread_id: ThreadId = session_id.into();
            return Ok(root_thread_id);
        }
        if stored_thread.parent_thread_id.is_none() {
            return Ok(thread_id);
        }
        Err(internal_error(format!(
            "persisted subagent thread {thread_id} is missing its root session metadata"
        )))
    }
}

pub(crate) fn agent_mailbox_status(snapshot: AgentMailboxUnreadSnapshot) -> AgentMailboxStatus {
    AgentMailboxStatus {
        total: snapshot.total.try_into().unwrap_or_default(),
        progress: snapshot.progress.try_into().unwrap_or_default(),
        result: snapshot.result.try_into().unwrap_or_default(),
        action_required: snapshot.action_required.try_into().unwrap_or_default(),
        revision: snapshot.revision.try_into().unwrap_or_default(),
    }
}

pub(crate) fn empty_agent_mailbox_status() -> AgentMailboxStatus {
    AgentMailboxStatus {
        total: 0,
        progress: 0,
        result: 0,
        action_required: 0,
        revision: 0,
    }
}

fn parse_thread_id_for_request(thread_id: &str) -> Result<ThreadId, JSONRPCErrorError> {
    ThreadId::from_string(thread_id)
        .map_err(|error| invalid_request(format!("invalid thread id: {error}")))
}

fn session_id_from_history(
    history: &codex_thread_store::StoredThreadHistory,
) -> Option<SessionId> {
    history.items.iter().find_map(|item| match item {
        RolloutItem::SessionMeta(meta) => Some(meta.meta.session_id),
        _ => None,
    })
}
