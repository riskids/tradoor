-- A take-profit counts only when price reached it after the entry: one already
-- past at the entry was being counted as reached. The live patterns are
-- weighed again under that rule at the next scan.
UPDATE signal SET outcome = NULL, outcome_at = NULL WHERE outcome IS NOT NULL;

-- Each pattern's limit entry: set by the scan that first saw it, half way back
-- from the furthest price since the third drive toward it, waiting two days;
-- where it filled (the bar, and the price), and how the trade went — or
-- `missed`, when price never came back or reached the first take-profit first.
ALTER TABLE signal
    ADD COLUMN limit_price      DOUBLE PRECISION,
    ADD COLUMN limit_until      BIGINT,
    ADD COLUMN limit_levels     JSONB,
    ADD COLUMN limit_at         BIGINT,
    ADD COLUMN limit_entry      DOUBLE PRECISION,
    ADD COLUMN limit_outcome    TEXT,
    ADD COLUMN limit_outcome_at BIGINT,
    ADD COLUMN limit_ret        DOUBLE PRECISION,
    ADD COLUMN limit_best       DOUBLE PRECISION,
    ADD COLUMN limit_worst      DOUBLE PRECISION,
    ADD COLUMN limit_told_at    BIGINT;

-- The replay keeps both kinds of entry: at the turn (`pattern`) and at the limit.
ALTER TABLE signal_backtest ADD COLUMN kind TEXT NOT NULL DEFAULT 'pattern';
ALTER TABLE signal_backtest DROP CONSTRAINT signal_backtest_pkey;
ALTER TABLE signal_backtest ADD PRIMARY KEY (run, kind, market, side, entry_at);

-- Telegram, for readers who turn it on, hears of the limit entries a scan sets.
ALTER TABLE signal_watch ADD COLUMN limits BOOLEAN NOT NULL DEFAULT FALSE;

-- Why a pattern left the list, for the list's Ended rows: a close past the
-- third drive, through the line, the first take-profit reached before the
-- turn, no turn in two days, two days since it — or gone, its coin unread.
ALTER TABLE signal ADD COLUMN ended_why TEXT;

-- The rules a scan ran under: a restart under new ones scans the bar again.
ALTER TABLE signal_scan ADD COLUMN version INTEGER;
