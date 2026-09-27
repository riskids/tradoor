-- Volume by price inside each minute: what the buyers took at each level and
-- what the sellers did, for the footprint chart. One row a minute a market,
-- the levels packed into one blob — a row per level would be two million rows
-- a month for one market, and ten times the bytes.
--
-- `base` is the lowest level in the bar, as a count of ticks; the blob holds
-- the levels above it. A month is kept; older minutes are trimmed.
CREATE TABLE footprint_minute (
    market TEXT NOT NULL,
    minute BIGINT NOT NULL,
    -- The price a level is worth, so a bar can be read without the market's settings.
    tick   DOUBLE PRECISION NOT NULL,
    base   BIGINT NOT NULL,
    -- Level deltas and volumes as varints; see bins/server/src/footprint.rs.
    levels BYTEA NOT NULL,
    trades BIGINT NOT NULL,
    PRIMARY KEY (market, minute)
);
CREATE INDEX footprint_minute_time_idx ON footprint_minute (minute);
