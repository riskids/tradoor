-- A browser signed in to an account holds a session: a random token of its own,
-- of which only the hash is kept here. Signing out of that browser deletes its
-- row; signing out everywhere, every row of the account's.
CREATE TABLE account_session (
    token_hash    TEXT PRIMARY KEY,
    owner         TEXT NOT NULL,
    telegram_user BIGINT NOT NULL,
    created_at    BIGINT NOT NULL,
    -- The browser it was made for, as its User-Agent named it, for a list of devices.
    agent         TEXT
);
CREATE INDEX account_session_owner_idx ON account_session (owner);
