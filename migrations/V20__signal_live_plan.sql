-- The live record moves to plan C: take-profits back down the third drive's
-- leg, the stop an ATR past its extreme. Each trade keeps its own prices, set
-- when it is entered, and what was weighed under the old plan is weighed again.
ALTER TABLE signal ADD COLUMN levels JSONB;
ALTER TABLE signal_setup
    ADD COLUMN levels   JSONB,
    ADD COLUMN pullback DOUBLE PRECISION;
UPDATE signal SET outcome = NULL, outcome_at = NULL, ret = NULL, best = NULL, worst = NULL, target = NULL, stop = NULL;
UPDATE signal_setup SET outcome = NULL, outcome_at = NULL, ret = NULL, best = NULL, worst = NULL, target = NULL;
