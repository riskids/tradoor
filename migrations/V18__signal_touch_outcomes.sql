-- How price at a setup's line went: what it aimed at, whether the two drives
-- became a pattern (and turned) within four days, and target or breakout first.
ALTER TABLE signal_setup
    ADD COLUMN target     DOUBLE PRECISION,
    ADD COLUMN became     TEXT,
    ADD COLUMN outcome    TEXT,
    ADD COLUMN outcome_at BIGINT,
    ADD COLUMN ret        DOUBLE PRECISION,
    ADD COLUMN best       DOUBLE PRECISION,
    ADD COLUMN worst      DOUBLE PRECISION;

-- The same, replayed over the history with the patterns.
CREATE TABLE signal_touch_backtest (
    run        BIGINT NOT NULL REFERENCES signal_backtest_run (at) ON DELETE CASCADE,
    market     TEXT NOT NULL,
    side       TEXT NOT NULL,
    touched_at BIGINT NOT NULL,
    entry      DOUBLE PRECISION NOT NULL,
    target     DOUBLE PRECISION NOT NULL,
    became     TEXT NOT NULL,
    outcome    TEXT NOT NULL,
    bars       INTEGER NOT NULL,
    ret        DOUBLE PRECISION NOT NULL,
    best       DOUBLE PRECISION NOT NULL,
    worst      DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (run, market, side, touched_at)
);
