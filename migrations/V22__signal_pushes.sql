-- The push into each drive, where the exchange reports who took each trade:
-- what takers bought less what they sold, in dollars and as a part of the
-- volume — [{usd, share}], the panel's numbers in place of the drives' prices.
-- A setup keeps the pushes into its two drives, and the push into its line
-- once price reaches it.
ALTER TABLE signal ADD COLUMN deltas JSONB;
ALTER TABLE signal_setup ADD COLUMN deltas JSONB;
