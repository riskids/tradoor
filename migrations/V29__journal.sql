-- The trading journal: every long or short position drawn on a chart is a
-- trade here, kept in step with its drawing, and a trade can be added by hand.
-- Deleting the drawing leaves the trade, no longer tied to it.
CREATE TABLE journal_trade (
    id           BIGSERIAL PRIMARY KEY,
    owner        TEXT NOT NULL,
    drawing_id   BIGINT UNIQUE REFERENCES drawing (id) ON DELETE SET NULL,
    market       TEXT NOT NULL,
    side         TEXT NOT NULL,
    -- The bar length it was drawn on; empty for a trade added by hand.
    timeframe    TEXT NOT NULL DEFAULT '',
    -- From when price is weighed against it: the position's left edge, or the
    -- moment it was drawn when that edge is the bar still forming.
    start_at     BIGINT NOT NULL,
    entry        DOUBLE PRECISION NOT NULL,
    stop         DOUBLE PRECISION NOT NULL,
    target       DOUBLE PRECISION NOT NULL,
    -- What the position risks, in the quote, and the coins that buys.
    risk         DOUBLE PRECISION NOT NULL,
    qty          DOUBLE PRECISION NOT NULL,
    -- planned (entry not reached), open, win (target), loss (stop), closed (by
    -- hand at `exit`), breakeven, cancelled.
    status       TEXT NOT NULL DEFAULT 'planned',
    opened_at    BIGINT,
    closed_at    BIGINT,
    exit         DOUBLE PRECISION,
    fees         DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- Whether price decides the status; a status set by hand turns it off.
    auto         BOOLEAN NOT NULL DEFAULT TRUE,
    -- How far price has been weighed: the open of the next minute to read.
    checked_until BIGINT NOT NULL,
    strategy     TEXT NOT NULL DEFAULT '',
    notes        TEXT NOT NULL DEFAULT '',
    created_at   BIGINT NOT NULL,
    updated_at   BIGINT NOT NULL
);
CREATE INDEX journal_trade_owner_idx ON journal_trade (owner, start_at DESC);
CREATE INDEX journal_trade_watch_idx ON journal_trade (checked_until) WHERE auto AND status IN ('planned', 'open');

-- A week's review: the answers to its questions, by ISO week (`2026-W42`).
CREATE TABLE journal_review (
    owner    TEXT NOT NULL,
    week     TEXT NOT NULL,
    answers  JSONB NOT NULL,
    saved_at BIGINT NOT NULL,
    PRIMARY KEY (owner, week)
);
