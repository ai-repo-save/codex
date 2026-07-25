CREATE TABLE agent_mailbox_deliveries (
    id TEXT PRIMARY KEY NOT NULL,
    root_thread_id TEXT NOT NULL,
    recipient_thread_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK(sequence > 0),
    FOREIGN KEY(root_thread_id, recipient_thread_id)
        REFERENCES agent_mailbox_recipients(root_thread_id, recipient_thread_id)
        ON DELETE CASCADE
);

INSERT INTO agent_mailbox_deliveries (
    id,
    root_thread_id,
    recipient_thread_id,
    sequence
)
SELECT
    id,
    root_thread_id,
    recipient_thread_id,
    sequence
FROM agent_mailbox_messages;

CREATE INDEX agent_mailbox_deliveries_recipient
    ON agent_mailbox_deliveries(root_thread_id, recipient_thread_id);
