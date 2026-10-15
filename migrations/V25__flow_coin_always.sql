-- A coin may be followed until an admin stops it: no end, as BTC has none.
ALTER TABLE flow_coin ALTER COLUMN ends_at DROP NOT NULL;
