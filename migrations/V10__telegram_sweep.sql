-- Messages the bot sent, so it can take them back. Telegram lets a bot delete
-- its own message for 48 hours and not a minute longer, so a window past that
-- is refused and a row left behind is dropped unsent rather than retried.
-- Two days by default, the most Telegram allows, so a chat of alerts does not
-- grow forever; Settings can turn it off.
ALTER TABLE telegram_chat ADD COLUMN clear_after_hours INTEGER DEFAULT 47;

CREATE TABLE telegram_message (
    chat_id    BIGINT NOT NULL,
    message_id BIGINT NOT NULL,
    clear_at   BIGINT NOT NULL,
    PRIMARY KEY (chat_id, message_id)
);
CREATE INDEX telegram_message_due_idx ON telegram_message (clear_at);
