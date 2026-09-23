-- A market's candle chart and its TPO chart keep separate drawings: a level
-- drawn over candles means nothing over profiles, and the other way round.
ALTER TABLE drawing ADD COLUMN chart TEXT NOT NULL DEFAULT 'candles' CHECK (chart IN ('candles', 'tpo'));
DROP INDEX drawing_owner_market_idx;
CREATE INDEX drawing_owner_market_idx ON drawing (owner, market, chart, id);
