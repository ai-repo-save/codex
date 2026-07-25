use super::StateRuntime;
use crate::AgentMailboxCategory;
use crate::AgentMailboxMessageInput;
use crate::AgentMailboxPayload;
use crate::AgentMailboxReadRequest;
use crate::AgentMailboxUnreadSnapshot;
use crate::runtime::test_support::test_thread_metadata;
use crate::runtime::test_support::unique_temp_dir;
use chrono::DateTime;
use chrono::Utc;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;

const ROOT_THREAD_ID: &str = "00000000-0000-0000-0000-000000000501";
const SENDER_THREAD_ID: &str = "00000000-0000-0000-0000-000000000502";
const RECIPIENT_THREAD_ID: &str = "00000000-0000-0000-0000-000000000503";
const SECOND_SENDER_THREAD_ID: &str = "00000000-0000-0000-0000-000000000504";
const ROOT_AGENT_PATH: &str = "/root";
const SENDER_AGENT_PATH: &str = "/root/worker";
const SECOND_SENDER_AGENT_PATH: &str = "/root/reviewer";
const MESSAGE_ONE_ID: &str = "00000000-0000-0000-0000-000000000511";
const MESSAGE_TWO_ID: &str = "00000000-0000-0000-0000-000000000512";
const MESSAGE_THREE_ID: &str = "00000000-0000-0000-0000-000000000513";
const MESSAGE_ONE_CONTENT: &str = "first progress";
const MESSAGE_TWO_CONTENT: &str = "review result";
const MESSAGE_THREE_CONTENT: &str = "follow-up required";

fn thread_id(value: &str) -> ThreadId {
    ThreadId::from_string(value).expect("valid thread id")
}

fn timestamp(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
}

fn message(
    id: &str,
    sender_thread_id: ThreadId,
    sender_agent_path: &str,
    category: AgentMailboxCategory,
    content: &str,
    created_at: DateTime<Utc>,
) -> AgentMailboxMessageInput {
    AgentMailboxMessageInput {
        id: id.to_string(),
        root_thread_id: thread_id(ROOT_THREAD_ID),
        sender_thread_id,
        sender_agent_path: sender_agent_path.to_string(),
        recipient_thread_id: thread_id(RECIPIENT_THREAD_ID),
        recipient_agent_path: ROOT_AGENT_PATH.to_string(),
        category,
        payload: AgentMailboxPayload::Plaintext {
            content: content.to_string(),
        },
        created_at,
    }
}

fn read_request(category: Option<AgentMailboxCategory>) -> AgentMailboxReadRequest {
    AgentMailboxReadRequest {
        root_thread_id: thread_id(ROOT_THREAD_ID),
        recipient_thread_id: thread_id(RECIPIENT_THREAD_ID),
        sender_thread_id: None,
        sender_agent_path: None,
        category,
        limit: 10,
    }
}

fn expected_snapshot(
    total: i64,
    progress: i64,
    result: i64,
    action_required: i64,
    revision: i64,
) -> AgentMailboxUnreadSnapshot {
    AgentMailboxUnreadSnapshot {
        total,
        progress,
        result,
        action_required,
        revision,
    }
}

#[tokio::test]
async fn mailbox_reads_filtered_messages_in_global_arrival_order() -> anyhow::Result<()> {
    let runtime = StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await?;
    let mailbox = runtime.agent_mailbox();
    mailbox
        .enqueue(message(
            MESSAGE_ONE_ID,
            thread_id(SENDER_THREAD_ID),
            SENDER_AGENT_PATH,
            AgentMailboxCategory::Progress,
            MESSAGE_ONE_CONTENT,
            timestamp(/*seconds*/ 1_700_000_001),
        ))
        .await?;
    let mut encrypted_message = message(
        MESSAGE_TWO_ID,
        thread_id(SECOND_SENDER_THREAD_ID),
        SECOND_SENDER_AGENT_PATH,
        AgentMailboxCategory::Result,
        MESSAGE_TWO_CONTENT,
        timestamp(/*seconds*/ 1_700_000_002),
    );
    encrypted_message.payload = AgentMailboxPayload::Encrypted {
        encrypted_content: MESSAGE_TWO_CONTENT.to_string(),
    };
    mailbox.enqueue(encrypted_message).await?;
    mailbox
        .enqueue(message(
            MESSAGE_THREE_ID,
            thread_id(SENDER_THREAD_ID),
            SENDER_AGENT_PATH,
            AgentMailboxCategory::ActionRequired,
            MESSAGE_THREE_CONTENT,
            timestamp(/*seconds*/ 1_700_000_003),
        ))
        .await?;

    let result = mailbox
        .consume(read_request(Some(AgentMailboxCategory::ActionRequired)))
        .await?;

    assert_eq!(
        vec![MESSAGE_THREE_ID.to_string()],
        result
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        expected_snapshot(
            /*total*/ 2,
            /*progress*/ 1,
            /*result*/ 1,
            /*action_required*/ 0,
            /*revision*/ 4,
        ),
        result.snapshot
    );

    let remaining = mailbox.consume(read_request(/*category*/ None)).await?;
    assert_eq!(
        vec![MESSAGE_ONE_ID.to_string(), MESSAGE_TWO_ID.to_string()],
        remaining
            .messages
            .iter()
            .map(|message| message.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        AgentMailboxPayload::Encrypted {
            encrypted_content: MESSAGE_TWO_CONTENT.to_string(),
        },
        remaining.messages[1].payload
    );
    assert_eq!(
        expected_snapshot(
            /*total*/ 0,
            /*progress*/ 0,
            /*result*/ 0,
            /*action_required*/ 0,
            /*revision*/ 5,
        ),
        remaining.snapshot
    );
    Ok(())
}

#[tokio::test]
async fn mailbox_enqueue_is_idempotent_after_runtime_restart() -> anyhow::Result<()> {
    let codex_home = unique_temp_dir();
    let first_runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let input = message(
        MESSAGE_ONE_ID,
        thread_id(SENDER_THREAD_ID),
        SENDER_AGENT_PATH,
        AgentMailboxCategory::Result,
        MESSAGE_ONE_CONTENT,
        timestamp(/*seconds*/ 1_700_000_001),
    );
    let inserted = first_runtime.agent_mailbox().enqueue(input.clone()).await?;
    assert_eq!(true, inserted.inserted);
    first_runtime.close().await;

    let resumed_runtime = StateRuntime::init(codex_home, "test-provider".to_string()).await?;
    let duplicate = resumed_runtime.agent_mailbox().enqueue(input).await?;
    assert_eq!(false, duplicate.inserted);
    assert_eq!(inserted.message, duplicate.message);
    assert_eq!(inserted.snapshot, duplicate.snapshot);

    let consumed = resumed_runtime
        .agent_mailbox()
        .consume(read_request(/*category*/ None))
        .await?;
    assert_eq!(vec![inserted.message], consumed.messages);
    assert_eq!(
        expected_snapshot(
            /*total*/ 0,
            /*progress*/ 0,
            /*result*/ 0,
            /*action_required*/ 0,
            /*revision*/ 2,
        ),
        consumed.snapshot
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_mailbox_reads_do_not_return_the_same_message() -> anyhow::Result<()> {
    let runtime = StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await?;
    let mailbox = runtime.agent_mailbox();
    mailbox
        .enqueue(message(
            MESSAGE_ONE_ID,
            thread_id(SENDER_THREAD_ID),
            SENDER_AGENT_PATH,
            AgentMailboxCategory::Progress,
            MESSAGE_ONE_CONTENT,
            timestamp(/*seconds*/ 1_700_000_001),
        ))
        .await?;
    mailbox
        .enqueue(message(
            MESSAGE_TWO_ID,
            thread_id(SECOND_SENDER_THREAD_ID),
            SECOND_SENDER_AGENT_PATH,
            AgentMailboxCategory::Result,
            MESSAGE_TWO_CONTENT,
            timestamp(/*seconds*/ 1_700_000_002),
        ))
        .await?;

    let first_reader = mailbox.clone();
    let second_reader = mailbox.clone();
    let mut first_request = read_request(/*category*/ None);
    first_request.limit = 1;
    let second_request = first_request.clone();
    let (first, second) = tokio::join!(
        first_reader.consume(first_request),
        second_reader.consume(second_request)
    );
    let mut message_ids = first?
        .messages
        .into_iter()
        .chain(second?.messages)
        .map(|message| message.id)
        .collect::<Vec<_>>();
    message_ids.sort();

    assert_eq!(
        vec![MESSAGE_ONE_ID.to_string(), MESSAGE_TWO_ID.to_string()],
        message_ids
    );
    assert_eq!(
        expected_snapshot(
            /*total*/ 0,
            /*progress*/ 0,
            /*result*/ 0,
            /*action_required*/ 0,
            /*revision*/ 4,
        ),
        mailbox
            .unread_snapshot(thread_id(ROOT_THREAD_ID), thread_id(RECIPIENT_THREAD_ID))
            .await?
    );
    Ok(())
}

#[tokio::test]
async fn deleting_recipient_thread_removes_its_mailbox() -> anyhow::Result<()> {
    let codex_home = unique_temp_dir();
    let runtime = StateRuntime::init(codex_home.clone(), "test-provider".to_string()).await?;
    let recipient_thread_id = thread_id(RECIPIENT_THREAD_ID);
    runtime
        .upsert_thread(&test_thread_metadata(
            &codex_home,
            recipient_thread_id,
            codex_home.clone(),
        ))
        .await?;
    runtime
        .agent_mailbox()
        .enqueue(message(
            MESSAGE_ONE_ID,
            thread_id(SENDER_THREAD_ID),
            SENDER_AGENT_PATH,
            AgentMailboxCategory::Result,
            MESSAGE_ONE_CONTENT,
            timestamp(/*seconds*/ 1_700_000_001),
        ))
        .await?;

    runtime.delete_thread(recipient_thread_id).await?;

    assert_eq!(
        expected_snapshot(
            /*total*/ 0,
            /*progress*/ 0,
            /*result*/ 0,
            /*action_required*/ 0,
            /*revision*/ 0,
        ),
        runtime
            .agent_mailbox()
            .unread_snapshot(thread_id(ROOT_THREAD_ID), recipient_thread_id)
            .await?
    );
    Ok(())
}
