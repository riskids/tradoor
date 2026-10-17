-- Each market-cap series rolled up by the hour, kept current each minute: a
-- day's, week's or month's bars read twenty-four rows a day instead of a
-- thousand and more, however long the recording grows.
CREATE TABLE index_hour (
    symbol    TEXT NOT NULL,
    open_time BIGINT NOT NULL,
    open      DOUBLE PRECISION NOT NULL,
    high      DOUBLE PRECISION NOT NULL,
    low       DOUBLE PRECISION NOT NULL,
    close     DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (symbol, open_time)
);
