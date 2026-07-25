use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicBool;

use codex_core::ThreadManager;
use codex_extension_api::ExtensionData;
use codex_extension_api::TerminalMessageContributor;
use codex_extension_api::TerminalMessageDisposition;
use codex_extension_api::TerminalMessageInput;
use codex_protocol::AgentPath;
use codex_protocol::ResponseItemId;
use codex_protocol::SessionId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::InterAgentCommunication;
use codex_state::AgentMailboxCategory;
use codex_state::AgentMailboxReadRequest;
use codex_state::StateRuntime;
use pretty_assertions::assert_eq;

use super::AgentMailboxExtension;
use super::AgentMailboxRuntime;
use crate::MAX_AGENT_MAILBOX_PAYLOAD_BYTES;
use crate::NoopAgentMailboxStatusNotifier;

const SENDER_THREAD_ID: &str = "00000000-0000-0000-0000-000000000701";
const RECIPIENT_THREAD_ID: &str = "00000000-0000-0000-0000-000000000702";
const TERMINAL_MESSAGE_ID: &str = "message-terminal-capture";
const TERMINAL_MESSAGE_BODY: &str = "terminal child result";

#[tokio::test]
async fn terminal_capture_claims_only_after_persisting_the_completed_message() -> anyhow::Result<()>
{
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(temporary_home.path().to_path_buf(), "test".to_string()).await?;
    let session_id = SessionId::new();
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    let recipient_thread_id = thread_id(RECIPIENT_THREAD_ID);
    let recipient_store = enabled_recipient_store(session_id, recipient_thread_id);
    let extension = AgentMailboxExtension::new(
        Arc::clone(&state),
        Weak::<ThreadManager>::new(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let communication = terminal_communication();
    let status = AgentStatus::Completed(/*final_message*/ None);

    assert_eq!(
        TerminalMessageDisposition::Committed,
        extension
            .contribute(TerminalMessageInput {
                session_id,
                sender_thread_id,
                recipient_thread_id,
                communication: &communication,
                status: &status,
                recipient_thread_store: &recipient_store,
            })
            .await
            .map_err(anyhow::Error::msg)?
    );
    let outcome = state
        .agent_mailbox()
        .consume(AgentMailboxReadRequest {
            root_thread_id: session_id.into(),
            recipient_thread_id,
            sender_thread_id: None,
            sender_agent_path: None,
            category: None,
            limit: 1,
        })
        .await?;
    assert_eq!(
        vec![AgentMailboxCategory::Result],
        outcome
            .messages
            .iter()
            .map(|message| message.category)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        vec![TERMINAL_MESSAGE_BODY.to_string()],
        outcome
            .messages
            .iter()
            .map(|message| match &message.payload {
                codex_state::AgentMailboxPayload::Plaintext { content } => content.clone(),
                codex_state::AgentMailboxPayload::Encrypted { .. } => {
                    panic!("plaintext terminal message should remain plaintext")
                }
            })
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[tokio::test]
async fn terminal_capture_leaves_disabled_or_ephemeral_recipients_unclaimed() -> anyhow::Result<()>
{
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(temporary_home.path().to_path_buf(), "test".to_string()).await?;
    let session_id = SessionId::new();
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    let recipient_thread_id = thread_id(RECIPIENT_THREAD_ID);
    let extension = AgentMailboxExtension::new(
        state,
        Weak::<ThreadManager>::new(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let communication = terminal_communication();
    let status = AgentStatus::Completed(/*final_message*/ None);
    let missing_runtime_store = ExtensionData::new(recipient_thread_id.to_string());
    let disabled_store = ExtensionData::new(recipient_thread_id.to_string());
    disabled_store.insert(AgentMailboxRuntime {
        thread_id: recipient_thread_id,
        root_thread_id: session_id.into(),
        agent_path: AgentPath::root(),
        persistent_thread_state_available: true,
        enabled: AtomicBool::new(false),
    });

    for recipient_thread_store in [&missing_runtime_store, &disabled_store] {
        assert_eq!(
            TerminalMessageDisposition::Unclaimed,
            extension
                .contribute(TerminalMessageInput {
                    session_id,
                    sender_thread_id,
                    recipient_thread_id,
                    communication: &communication,
                    status: &status,
                    recipient_thread_store,
                })
                .await
                .map_err(anyhow::Error::msg)?
        );
    }
    Ok(())
}

#[tokio::test]
async fn terminal_capture_enforces_the_encrypted_payload_budget() -> anyhow::Result<()> {
    let temporary_home = tempfile::tempdir()?;
    let state = StateRuntime::init(temporary_home.path().to_path_buf(), "test".to_string()).await?;
    let session_id = SessionId::new();
    let sender_thread_id = thread_id(SENDER_THREAD_ID);
    let recipient_thread_id = thread_id(RECIPIENT_THREAD_ID);
    let recipient_store = enabled_recipient_store(session_id, recipient_thread_id);
    let extension = AgentMailboxExtension::new(
        Arc::clone(&state),
        Weak::<ThreadManager>::new(),
        Arc::new(NoopAgentMailboxStatusNotifier),
    );
    let status = AgentStatus::Completed(/*final_message*/ None);
    let mut accepted_communication = terminal_communication();
    accepted_communication.encrypted_content = Some("x".repeat(MAX_AGENT_MAILBOX_PAYLOAD_BYTES));

    assert_eq!(
        TerminalMessageDisposition::Committed,
        extension
            .contribute(TerminalMessageInput {
                session_id,
                sender_thread_id,
                recipient_thread_id,
                communication: &accepted_communication,
                status: &status,
                recipient_thread_store: &recipient_store,
            })
            .await
            .map_err(anyhow::Error::msg)?
    );
    assert_eq!(
        1,
        state
            .agent_mailbox()
            .unread_snapshot(session_id.into(), recipient_thread_id)
            .await?
            .total
    );

    let mut oversized_communication = terminal_communication();
    oversized_communication.id = Some(ResponseItemId::from_server(
        "message-terminal-oversized".to_string(),
    ));
    oversized_communication.encrypted_content =
        Some("x".repeat(MAX_AGENT_MAILBOX_PAYLOAD_BYTES + 1));
    let Err(error) = extension
        .contribute(TerminalMessageInput {
            session_id,
            sender_thread_id,
            recipient_thread_id,
            communication: &oversized_communication,
            status: &status,
            recipient_thread_store: &recipient_store,
        })
        .await
    else {
        panic!("oversized encrypted terminal message should not be captured");
    };
    assert_eq!(
        format!(
            "failed to capture terminal agent mailbox message: agent mailbox message exceeds the {MAX_AGENT_MAILBOX_PAYLOAD_BYTES}-byte limit"
        ),
        error
    );
    assert_eq!(
        1,
        state
            .agent_mailbox()
            .unread_snapshot(session_id.into(), recipient_thread_id)
            .await?
            .total
    );
    Ok(())
}

fn enabled_recipient_store(session_id: SessionId, recipient_thread_id: ThreadId) -> ExtensionData {
    let store = ExtensionData::new(recipient_thread_id.to_string());
    store.insert(AgentMailboxRuntime {
        thread_id: recipient_thread_id,
        root_thread_id: session_id.into(),
        agent_path: AgentPath::root(),
        persistent_thread_state_available: true,
        enabled: AtomicBool::new(true),
    });
    store
}

fn terminal_communication() -> InterAgentCommunication {
    let mut communication = InterAgentCommunication::new(
        AgentPath::root()
            .join("worker")
            .expect("test sender path should be valid"),
        AgentPath::root(),
        Vec::new(),
        TERMINAL_MESSAGE_BODY.to_string(),
        /*trigger_turn*/ false,
    );
    communication.id = Some(ResponseItemId::from_server(TERMINAL_MESSAGE_ID.to_string()));
    communication
}

fn thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("mailbox test thread ID should be valid")
}
