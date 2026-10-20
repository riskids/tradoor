-- A position's partial take profits, partial cut losses and added entries:
-- the levels drawn with it (`legs`: a kind, a price and a share of the size
-- the entry buys), and what has been filled so far (`fills`: the entry, each
-- leg, the stop or the target, or a close by hand — a price, the coins and
-- when). What a trade made is worked out from its fills.
ALTER TABLE journal_trade ADD COLUMN legs JSONB NOT NULL DEFAULT '[]';
ALTER TABLE journal_trade ADD COLUMN fills JSONB NOT NULL DEFAULT '[]';
