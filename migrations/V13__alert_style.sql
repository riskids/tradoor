-- How an alert's line is drawn on the chart: colour (opacity included), dash
-- and width. Empty is the chart's default, as every alert before this had.
ALTER TABLE alert ADD COLUMN style JSONB NOT NULL DEFAULT '{}';
