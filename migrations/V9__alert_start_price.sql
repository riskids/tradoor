-- The market price when an alert was set, so a trigger can say which way the
-- price came: a level put above the price and reached is a rise, one put below
-- and reached is a fall. Null on alerts made before this was kept.
ALTER TABLE alert ADD COLUMN start_price DOUBLE PRECISION;
ALTER TABLE alert_log ADD COLUMN start_price DOUBLE PRECISION;
