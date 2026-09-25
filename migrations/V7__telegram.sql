-- Telegram chats an alert is sent to, per browser id like the alerts are.
-- A bot cannot start a conversation, so the reader starts it from a link
-- carrying a one-use code; the code says which browser the chat belongs to.
CREATE TABLE telegram_chat (
    owner     TEXT NOT NULL,
    chat_id   BIGINT NOT NULL,
    name      TEXT,
    linked_at BIGINT NOT NULL,
    PRIMARY KEY (owner, chat_id)
);
CREATE INDEX telegram_chat_owner_idx ON telegram_chat (owner);

CREATE TABLE telegram_link (
    code       TEXT PRIMARY KEY,
    owner      TEXT NOT NULL,
    created_at BIGINT NOT NULL
);
