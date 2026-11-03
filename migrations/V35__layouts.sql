-- Named chart layouts, as on TradingView: a workspace is one layout, listed,
-- starred and last-opened; its drawings are its own (`layout` = workspace id).
-- Rows from before layouts belong to 'default'.
ALTER TABLE workspace ADD COLUMN starred   BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE workspace ADD COLUMN opened_at TIMESTAMPTZ;
ALTER TABLE workspace ADD COLUMN summary   TEXT NOT NULL DEFAULT '';

ALTER TABLE drawing ADD COLUMN layout TEXT NOT NULL DEFAULT 'default';
DROP INDEX drawing_owner_market_idx;
CREATE INDEX drawing_owner_market_idx ON drawing (owner, layout, market, chart, id);
