-- Drawings and workspaces belong to one browser: a random id it keeps and
-- sends with every request. Rows from before owners existed have the empty
-- owner — nobody's — until `tradoor-server adopt` hands them to one.
ALTER TABLE drawing ADD COLUMN owner TEXT NOT NULL DEFAULT '';
DROP INDEX drawing_chart_idx;
CREATE INDEX drawing_owner_market_idx ON drawing (owner, market, id);

ALTER TABLE workspace ADD COLUMN owner TEXT NOT NULL DEFAULT '';
ALTER TABLE workspace DROP CONSTRAINT workspace_pkey;
ALTER TABLE workspace ADD PRIMARY KEY (owner, id);
