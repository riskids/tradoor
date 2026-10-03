-- Two drives waiting for a third at their line, as each scan found them, and
-- the moment live prices reached the line; who is told when one does.
CREATE TABLE signal_setup (
    market      TEXT NOT NULL,
    side        TEXT NOT NULL,
    first_at    BIGINT NOT NULL,
    second_at   BIGINT NOT NULL,
    -- [{at, price}] for the first two drives.
    drives      JSONB NOT NULL,
    atr         DOUBLE PRECISION NOT NULL,
    -- Where a third drive starts: past the second by half an ATR.
    clear       DOUBLE PRECISION NOT NULL,
    until_at    BIGINT NOT NULL,
    seen_bar    BIGINT NOT NULL,
    touched_at  BIGINT,
    touch_price DOUBLE PRECISION,
    told_at     BIGINT,
    PRIMARY KEY (market, side, first_at, second_at)
);
CREATE INDEX signal_setup_seen_idx ON signal_setup (seen_bar);

ALTER TABLE signal_watch ADD COLUMN touch BOOLEAN NOT NULL DEFAULT FALSE;
