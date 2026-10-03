-- The Signal panel's screener: each scan, every pattern a scan found (kept
-- with the last scan that still saw it), and who is told when one turns.
CREATE TABLE signal_scan (
    -- The close of the newest bar the scan read.
    bar     BIGINT PRIMARY KEY,
    at      BIGINT NOT NULL,
    markets INTEGER NOT NULL,
    found   INTEGER NOT NULL
);

CREATE TABLE signal (
    id         BIGSERIAL PRIMARY KEY,
    kind       TEXT NOT NULL,
    market     TEXT NOT NULL,
    side       TEXT NOT NULL,
    timeframe  TEXT NOT NULL,
    -- One pattern is one third drive: later scans update it rather than add.
    third_at   BIGINT NOT NULL,
    -- [{at, price}] for each drive, oldest first.
    drives     JSONB NOT NULL,
    turned_at  BIGINT,
    turn_price DOUBLE PRECISION,
    score      INTEGER NOT NULL,
    divergence BOOLEAN NOT NULL,
    price      DOUBLE PRECISION NOT NULL,
    found_bar  BIGINT NOT NULL,
    seen_bar   BIGINT NOT NULL,
    told_at    BIGINT,
    UNIQUE (kind, market, side, timeframe, third_at)
);
CREATE INDEX signal_seen_idx ON signal (seen_bar);

CREATE TABLE signal_watch (
    owner  TEXT PRIMARY KEY,
    notify BOOLEAN NOT NULL
);
