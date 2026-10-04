-- A Telegram account a reader signs in with, and the browser id everything of
-- theirs is kept under: a browser that lost its id — history cleared, another
-- device — takes it back by signing in. Nothing moves: drawings, layouts,
-- alerts and the rest stay keyed by that id.
CREATE TABLE account (
    telegram_user BIGINT PRIMARY KEY,
    owner         TEXT NOT NULL,
    name          TEXT,
    since         BIGINT NOT NULL
);
CREATE INDEX account_owner_idx ON account (owner);

-- A browser asking to sign in: the one-use code the bot's link carries, the id
-- it asked from and its clock; the Telegram user who opened the link, the only
-- one asked to confirm; and once they say yes, the account's id, held for the
-- browser that asked to collect once.
CREATE TABLE account_signin (
    code          TEXT PRIMARY KEY,
    asker         TEXT NOT NULL,
    created_at    BIGINT NOT NULL,
    utc_offset    INTEGER NOT NULL DEFAULT 0,
    zone          TEXT,
    telegram_user BIGINT,
    owner         TEXT
);

-- Chats linked for alerts already are accounts: a private chat's id is its
-- user's. Where one person linked several browsers, the account is the id with
-- the most drawings, then the first linked.
INSERT INTO account (telegram_user, owner, name, since)
SELECT DISTINCT ON (c.chat_id) c.chat_id, c.owner, c.name, c.linked_at
FROM telegram_chat c
WHERE c.chat_id > 0
ORDER BY c.chat_id, (SELECT count(*) FROM drawing d WHERE d.owner = c.owner) DESC, c.linked_at
ON CONFLICT DO NOTHING;
