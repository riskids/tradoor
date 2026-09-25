-- Net taker flow by the minute, and the crossings a reader was told about.
-- Two days of minutes is what the panel shows; older ones are trimmed.
CREATE TABLE flow_minute (
    market   TEXT NOT NULL,
    minute   BIGINT NOT NULL,
    buy_usd  DOUBLE PRECISION NOT NULL,
    sell_usd DOUBLE PRECISION NOT NULL,
    trades   BIGINT NOT NULL,
    -- Where it last traded that minute, so a mark on the chart has a price.
    price    DOUBLE PRECISION NOT NULL,
    PRIMARY KEY (market, minute)
);
CREATE INDEX flow_minute_time_idx ON flow_minute (minute);

-- Per reader, because the threshold that counts is theirs to set.
CREATE TABLE flow_watch (
    owner         TEXT NOT NULL,
    market        TEXT NOT NULL,
    threshold_usd DOUBLE PRECISION NOT NULL,
    notify        BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY (owner, market)
);

CREATE TABLE flow_event (
    id        BIGSERIAL PRIMARY KEY,
    owner     TEXT NOT NULL,
    market    TEXT NOT NULL,
    at        BIGINT NOT NULL,
    delta_usd DOUBLE PRECISION NOT NULL,
    window_ms INTEGER NOT NULL,
    price     DOUBLE PRECISION NOT NULL
);
CREATE INDEX flow_event_owner_idx ON flow_event (owner, at DESC);
