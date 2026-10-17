-- An index's value through the day, as its publisher's delayed quote moves:
-- the VIX's minutes, from which bars shorter than a day are drawn. `at` is the
-- quote's own time.
CREATE TABLE index_tick (
    symbol TEXT NOT NULL,
    at     BIGINT NOT NULL,
    value  DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (symbol, at)
);
