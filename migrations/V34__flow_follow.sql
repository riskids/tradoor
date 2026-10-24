-- Coins whose flow a reader follows besides BTC and ETH: each signed-in reader
-- their own list, of at most seven. The server records a coin once however
-- many follow it, while one of them is live and its reader was here lately.
CREATE TABLE flow_follow (
    owner    TEXT NOT NULL,
    coin     TEXT NOT NULL,
    added_at BIGINT NOT NULL,
    -- When it lapses; empty for one followed until it is stopped.
    ends_at  BIGINT,
    PRIMARY KEY (owner, coin)
);
CREATE INDEX flow_follow_coin_idx ON flow_follow (coin);

-- The coins the admins chose for everyone become the lists of whoever chose them.
INSERT INTO flow_follow (owner, coin, added_at, ends_at)
SELECT added_by, coin, added_at, ends_at FROM flow_coin;
DROP TABLE flow_coin;

-- When a browser signed in to the account last asked the server anything, to
-- the hour: a coin none of whose readers was here for two weeks is not recorded.
ALTER TABLE account ADD COLUMN seen_at BIGINT;
UPDATE account SET seen_at = (extract(epoch FROM now()) * 1000)::bigint;
