-- How each pattern played out: what it pointed at and where it was wrong,
-- where a scan first saw it turned, and whether price reached the target or
-- the stop first, within four days.
ALTER TABLE signal
    ADD COLUMN target     DOUBLE PRECISION,
    ADD COLUMN stop       DOUBLE PRECISION,
    ADD COLUMN entry_at   BIGINT,
    ADD COLUMN entry      DOUBLE PRECISION,
    ADD COLUMN outcome    TEXT,
    ADD COLUMN outcome_at BIGINT,
    ADD COLUMN ret        DOUBLE PRECISION,
    ADD COLUMN best       DOUBLE PRECISION,
    ADD COLUMN worst      DOUBLE PRECISION;
CREATE INDEX signal_open_idx ON signal (entry_at) WHERE outcome IS NULL;

-- The same rules replayed over months of history, bar by bar, seeing nothing
-- past the bar a scan would have read: what the record would have been.
CREATE TABLE signal_backtest_run (
    at      BIGINT PRIMARY KEY,
    -- The rules' version: a change to them asks for a fresh run.
    version INTEGER NOT NULL,
    markets INTEGER NOT NULL,
    from_at BIGINT NOT NULL,
    to_at   BIGINT NOT NULL
);

CREATE TABLE signal_backtest (
    run        BIGINT NOT NULL REFERENCES signal_backtest_run (at) ON DELETE CASCADE,
    market     TEXT NOT NULL,
    side       TEXT NOT NULL,
    entry_at   BIGINT NOT NULL,
    entry      DOUBLE PRECISION NOT NULL,
    target     DOUBLE PRECISION NOT NULL,
    stop       DOUBLE PRECISION NOT NULL,
    score      INTEGER NOT NULL,
    divergence BOOLEAN NOT NULL,
    outcome    TEXT NOT NULL,
    bars       INTEGER NOT NULL,
    ret        DOUBLE PRECISION NOT NULL,
    best       DOUBLE PRECISION NOT NULL,
    worst      DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (run, market, side, entry_at)
);
