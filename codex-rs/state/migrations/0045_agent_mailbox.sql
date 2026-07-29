CREATE TABLE agent_mailbox_recipients (
    root_thread_id TEXT NOT NULL,
    recipient_thread_id TEXT NOT NULL,
    recipient_agent_path TEXT NOT NULL,
    next_sequence INTEGER NOT NULL DEFAULT 1 CHECK(next_sequence > 0),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    PRIMARY KEY(root_thread_id, recipient_thread_id)
);

CREATE TABLE agent_mailbox_messages (
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
        ON DELETE CASCADE,
    UNIQUE(root_thread_id, recipient_thread_id, sequence)
);

CREATE INDEX agent_mailbox_messages_recipient_sequence
    ON agent_mailbox_messages(root_thread_id, recipient_thread_id, sequence);

CREATE INDEX agent_mailbox_messages_recipient_category_sequence
    ON agent_mailbox_messages(root_thread_id, recipient_thread_id, category, sequence);

CREATE INDEX agent_mailbox_messages_recipient_sender_sequence
    ON agent_mailbox_messages(root_thread_id, recipient_thread_id, sender_thread_id, sequence);
