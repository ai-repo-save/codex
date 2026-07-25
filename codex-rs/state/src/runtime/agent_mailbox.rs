use super::*;
use crate::AgentMailboxCategory;
use crate::AgentMailboxEnqueueOutcome;
use crate::AgentMailboxMessage;
use crate::AgentMailboxMessageInput;
use crate::AgentMailboxPayload;
use crate::AgentMailboxReadOutcome;
use crate::AgentMailboxReadRequest;
use crate::AgentMailboxUnreadSnapshot;
use crate::model::datetime_to_epoch_millis;
use crate::model::epoch_millis_to_datetime;
use codex_protocol::ThreadId;
use sqlx::Row;
use sqlx::sqlite::SqliteRow;
use std::sync::Arc;

pub const MAX_AGENT_MAILBOX_READ_LIMIT: usize = 50;

#[derive(Clone)]
pub struct AgentMailboxStore {
    pool: Arc<SqlitePool>,
}

impl AgentMailboxStore {
    pub(crate) fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }

    pub async fn enqueue(
        &self,
        message: AgentMailboxMessageInput,
    ) -> anyhow::Result<AgentMailboxEnqueueOutcome> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            r#"
INSERT INTO agent_mailbox_recipients (
    root_thread_id,
    recipient_thread_id,
    recipient_agent_path,
    next_sequence,
    revision
)
SELECT ?, ?, ?, 1, 0
WHERE NOT EXISTS (
    SELECT 1 FROM agent_mailbox_deliveries WHERE id = ?
)
ON CONFLICT(root_thread_id, recipient_thread_id) DO UPDATE SET
    recipient_agent_path = excluded.recipient_agent_path
            "#,
        )
        .bind(message.root_thread_id.to_string())
        .bind(message.recipient_thread_id.to_string())
        .bind(&message.recipient_agent_path)
        .bind(&message.id)
        .execute(&mut *transaction)
        .await?;

        let reserved = sqlx::query(
            r#"
UPDATE agent_mailbox_recipients
SET
    next_sequence = next_sequence + 1,
    revision = revision + 1
WHERE root_thread_id = ?
  AND recipient_thread_id = ?
  AND NOT EXISTS (
      SELECT 1 FROM agent_mailbox_deliveries WHERE id = ?
  )
RETURNING next_sequence - 1 AS sequence
            "#,
        )
        .bind(message.root_thread_id.to_string())
        .bind(message.recipient_thread_id.to_string())
        .bind(&message.id)
        .fetch_optional(&mut *transaction)
        .await?;

        let Some(reserved) = reserved else {
            let existing = delivery_by_id(&mut transaction, &message.id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("agent mailbox idempotency lookup lost delivery"))?;
            let snapshot = unread_snapshot_in_transaction(
                &mut transaction,
                existing.root_thread_id,
                existing.recipient_thread_id,
            )
            .await?;
            transaction.commit().await?;
            return Ok(AgentMailboxEnqueueOutcome {
                inserted: false,
                message: existing,
                snapshot,
            });
        };
        let sequence: i64 = reserved.try_get("sequence")?;
        let (payload_kind, payload) = message.payload.kind_and_content();
        sqlx::query(
            r#"
INSERT INTO agent_mailbox_deliveries (
    id,
    root_thread_id,
    sender_thread_id,
    sender_agent_path,
    recipient_thread_id,
    recipient_agent_path,
    category,
    payload_kind,
    payload,
    created_at_ms,
    sequence
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&message.id)
        .bind(message.root_thread_id.to_string())
        .bind(message.sender_thread_id.to_string())
        .bind(&message.sender_agent_path)
        .bind(message.recipient_thread_id.to_string())
        .bind(&message.recipient_agent_path)
        .bind(message.category.as_str())
        .bind(payload_kind)
        .bind(payload)
        .bind(datetime_to_epoch_millis(message.created_at))
        .bind(sequence)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
INSERT INTO agent_mailbox_messages (
    id,
    root_thread_id,
    sender_thread_id,
    sender_agent_path,
    recipient_thread_id,
    recipient_agent_path,
    category,
    payload_kind,
    payload,
    created_at_ms,
    sequence
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&message.id)
        .bind(message.root_thread_id.to_string())
        .bind(message.sender_thread_id.to_string())
        .bind(&message.sender_agent_path)
        .bind(message.recipient_thread_id.to_string())
        .bind(&message.recipient_agent_path)
        .bind(message.category.as_str())
        .bind(payload_kind)
        .bind(payload)
        .bind(datetime_to_epoch_millis(message.created_at))
        .bind(sequence)
        .execute(&mut *transaction)
        .await?;

        let stored = AgentMailboxMessage {
            id: message.id,
            root_thread_id: message.root_thread_id,
            sender_thread_id: message.sender_thread_id,
            sender_agent_path: message.sender_agent_path,
            recipient_thread_id: message.recipient_thread_id,
            recipient_agent_path: message.recipient_agent_path,
            category: message.category,
            payload: message.payload,
            created_at: message.created_at,
            sequence,
        };
        let snapshot = unread_snapshot_in_transaction(
            &mut transaction,
            stored.root_thread_id,
            stored.recipient_thread_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(AgentMailboxEnqueueOutcome {
            inserted: true,
            message: stored,
            snapshot,
        })
    }

    pub async fn consume(
        &self,
        request: AgentMailboxReadRequest,
    ) -> anyhow::Result<AgentMailboxReadOutcome> {
        let limit = request.limit.min(MAX_AGENT_MAILBOX_READ_LIMIT);
        if limit == 0 {
            return Ok(AgentMailboxReadOutcome {
                messages: Vec::new(),
                snapshot: self
                    .unread_snapshot(request.root_thread_id, request.recipient_thread_id)
                    .await?,
            });
        }

        let sender_thread_id = request.sender_thread_id.map(|thread_id| thread_id.to_string());
        let category = request.category.map(AgentMailboxCategory::as_str);
        let mut transaction = self.pool.begin().await?;
        let rows = sqlx::query(
            r#"
DELETE FROM agent_mailbox_messages
WHERE id IN (
    SELECT id
    FROM agent_mailbox_messages
    WHERE root_thread_id = ?
      AND recipient_thread_id = ?
      AND (? IS NULL OR sender_thread_id = ?)
      AND (? IS NULL OR sender_agent_path = ?)
      AND (? IS NULL OR category = ?)
    ORDER BY sequence ASC
    LIMIT ?
)
RETURNING
    id,
    root_thread_id,
    sender_thread_id,
    sender_agent_path,
    recipient_thread_id,
    recipient_agent_path,
    category,
    payload_kind,
    payload,
    created_at_ms,
    sequence
            "#,
        )
        .bind(request.root_thread_id.to_string())
        .bind(request.recipient_thread_id.to_string())
        .bind(sender_thread_id.as_deref())
        .bind(sender_thread_id.as_deref())
        .bind(request.sender_agent_path.as_deref())
        .bind(request.sender_agent_path.as_deref())
        .bind(category)
        .bind(category)
        .bind(limit as i64)
        .fetch_all(&mut *transaction)
        .await?;
        let mut messages = rows
            .iter()
            .map(agent_mailbox_message_from_row)
            .collect::<anyhow::Result<Vec<_>>>()?;
        messages.sort_by_key(|message| message.sequence);
        if !messages.is_empty() {
            sqlx::query(
                r#"
UPDATE agent_mailbox_recipients
SET revision = revision + 1
WHERE root_thread_id = ? AND recipient_thread_id = ?
                "#,
            )
            .bind(request.root_thread_id.to_string())
            .bind(request.recipient_thread_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        let snapshot = unread_snapshot_in_transaction(
            &mut transaction,
            request.root_thread_id,
            request.recipient_thread_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(AgentMailboxReadOutcome { messages, snapshot })
    }

    pub async fn unread_snapshot(
        &self,
        root_thread_id: ThreadId,
        recipient_thread_id: ThreadId,
    ) -> anyhow::Result<AgentMailboxUnreadSnapshot> {
        unread_snapshot_in_pool(
            self.pool.as_ref(),
            root_thread_id,
            recipient_thread_id,
        )
        .await
    }

    pub async fn delete_recipient_messages(
        &self,
        recipient_thread_id: ThreadId,
    ) -> anyhow::Result<()> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM agent_mailbox_recipients WHERE recipient_thread_id = ?")
            .bind(recipient_thread_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn delivery_by_id(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> anyhow::Result<Option<AgentMailboxMessage>> {
    sqlx::query(
        r#"
SELECT
    id,
    root_thread_id,
    sender_thread_id,
    sender_agent_path,
    recipient_thread_id,
    recipient_agent_path,
    category,
    payload_kind,
    payload,
    created_at_ms,
    sequence
FROM agent_mailbox_deliveries
WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await?
    .map(|row| agent_mailbox_message_from_row(&row))
    .transpose()
}

async fn unread_snapshot_in_pool(
    pool: &SqlitePool,
    root_thread_id: ThreadId,
    recipient_thread_id: ThreadId,
) -> anyhow::Result<AgentMailboxUnreadSnapshot> {
    let mut transaction = pool.begin().await?;
    let snapshot =
        unread_snapshot_in_transaction(&mut transaction, root_thread_id, recipient_thread_id).await?;
    transaction.commit().await?;
    Ok(snapshot)
}

async fn unread_snapshot_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    root_thread_id: ThreadId,
    recipient_thread_id: ThreadId,
) -> anyhow::Result<AgentMailboxUnreadSnapshot> {
    let root_thread_id = root_thread_id.to_string();
    let recipient_thread_id = recipient_thread_id.to_string();
    let counts = sqlx::query(
        r#"
SELECT
    COUNT(*) AS total,
    COALESCE(SUM(category = 'progress'), 0) AS progress,
    COALESCE(SUM(category = 'result'), 0) AS result,
    COALESCE(SUM(category = 'action_required'), 0) AS action_required
FROM agent_mailbox_messages
WHERE root_thread_id = ? AND recipient_thread_id = ?
        "#,
    )
    .bind(&root_thread_id)
    .bind(&recipient_thread_id)
    .fetch_one(&mut **transaction)
    .await?;
    let revision = sqlx::query_scalar::<_, i64>(
        r#"
SELECT revision
FROM agent_mailbox_recipients
WHERE root_thread_id = ? AND recipient_thread_id = ?
        "#,
    )
    .bind(root_thread_id)
    .bind(recipient_thread_id)
    .fetch_optional(&mut **transaction)
    .await?
    .unwrap_or(0);
    Ok(AgentMailboxUnreadSnapshot {
        total: counts.try_get("total")?,
        progress: counts.try_get("progress")?,
        result: counts.try_get("result")?,
        action_required: counts.try_get("action_required")?,
        revision,
    })
}

fn agent_mailbox_message_from_row(row: &SqliteRow) -> anyhow::Result<AgentMailboxMessage> {
    let root_thread_id: String = row.try_get("root_thread_id")?;
    let sender_thread_id: String = row.try_get("sender_thread_id")?;
    let recipient_thread_id: String = row.try_get("recipient_thread_id")?;
    let category: String = row.try_get("category")?;
    let payload_kind: String = row.try_get("payload_kind")?;
    Ok(AgentMailboxMessage {
        id: row.try_get("id")?,
        root_thread_id: ThreadId::try_from(root_thread_id)?,
        sender_thread_id: ThreadId::try_from(sender_thread_id)?,
        sender_agent_path: row.try_get("sender_agent_path")?,
        recipient_thread_id: ThreadId::try_from(recipient_thread_id)?,
        recipient_agent_path: row.try_get("recipient_agent_path")?,
        category: AgentMailboxCategory::try_from(category.as_str())?,
        payload: AgentMailboxPayload::from_parts(&payload_kind, row.try_get("payload")?)?,
        created_at: epoch_millis_to_datetime(row.try_get("created_at_ms")?)?,
        sequence: row.try_get("sequence")?,
    })
}
