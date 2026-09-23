-- Price alerts, per browser id like drawings. The browser watches prices and
-- reports a trigger; the row keeps its state and the log keeps each trigger.
CREATE TABLE alert (
    id            BIGSERIAL PRIMARY KEY,
    owner         TEXT NOT NULL,
    market        TEXT NOT NULL,
    condition     TEXT NOT NULL CHECK (condition IN ('crossing', 'crossing_up', 'crossing_down', 'greater', 'less')),
    price         DOUBLE PRECISION NOT NULL,
    message       TEXT,
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    BIGINT NOT NULL,
    triggered_at  BIGINT,
    trigger_price DOUBLE PRECISION
);
CREATE INDEX alert_owner_idx ON alert (owner, id);

CREATE TABLE alert_log (
    id        BIGSERIAL PRIMARY KEY,
    owner     TEXT NOT NULL,
    alert_id  BIGINT REFERENCES alert (id) ON DELETE SET NULL,
    market    TEXT NOT NULL,
    condition TEXT NOT NULL,
    price     DOUBLE PRECISION NOT NULL,
    message   TEXT,
    time      BIGINT NOT NULL,
    value     DOUBLE PRECISION NOT NULL
);
CREATE INDEX alert_log_owner_idx ON alert_log (owner, time DESC);
