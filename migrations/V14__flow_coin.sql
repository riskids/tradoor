-- Coins whose flow is followed besides BTC, chosen by the server's admins.
-- Each is followed until a day after it was last asked for, then lapses.
CREATE TABLE flow_coin (
    coin     TEXT PRIMARY KEY,
    added_by TEXT NOT NULL,
    added_at BIGINT NOT NULL,
    ends_at  BIGINT NOT NULL
);
