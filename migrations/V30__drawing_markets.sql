-- A drawing's market is any chart's: a venue's market, an index the server
-- works out (`index:TOTAL`, no kind), or one priced in another
-- (`venue:A/venue:B`). The first rule took only the first, so drawing on an
-- index's chart or a ratio's failed when the row was written.
ALTER TABLE drawing DROP CONSTRAINT drawing_market_check;
ALTER TABLE drawing ADD CONSTRAINT drawing_market_check
    CHECK (market ~ '^[a-z]+(-(spot|perp))?:[A-Z0-9_]+(/[a-z]+(-(spot|perp))?:[A-Z0-9_]+)?$');
