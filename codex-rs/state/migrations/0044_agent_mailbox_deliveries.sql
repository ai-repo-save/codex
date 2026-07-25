CREATE TABLE agent_mailbox_deliveries (
    id TEXT PRIMARY KEY NOT NULL,
    root_thread_id TEXT NOT NULL,
    sender_thread_id TEXT NOT NULL,
    sender_agent_path TEXT NOT NULL,
    recipient_thread_id TEXT NOT NULL,
    recipient_agent_path TEXT NOT NULL,
    category TEXT NOT NULL CHECK(category IN ('progress', 'result', 'action_required')),
    payload_kind TEXT NOT NULL CHECK(payload_kind IN ('plaintext', 'encrypted')),
    payload TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    FOREIGN KEY(root_thread_id, recipient_thread_id)
        REFERENCES agent_mailbox_recipients(root_thread_id, recipient_thread_id)
        ON DELETE CASCADE
);

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
)
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
FROM agent_mailbox_messages;

CREATE INDEX agent_mailbox_deliveries_recipient
    ON agent_mailbox_deliveries(root_thread_id, recipient_thread_id);
