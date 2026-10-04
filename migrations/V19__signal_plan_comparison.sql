-- Each way of setting a trade's take-profits and stop, replayed side by side:
-- one JSON summary per run, by plan, kind (pattern or touch), side and group.
ALTER TABLE signal_backtest_run ADD COLUMN variants JSONB;
