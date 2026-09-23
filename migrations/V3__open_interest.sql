-- Open interest of a coin on each perpetual venue, in coins, one row a minute.
--
-- Recorded by the server every few seconds (the row of the current minute is
-- overwritten, so it holds the minute's last reading) and backfilled from the
-- five-minute history the exchanges that keep one publish. Kept for 35 days.
CREATE TABLE open_interest (
  coin   TEXT             NOT NULL CHECK (coin ~ '^[A-Z0-9]{1,20}$'),
  venue  TEXT             NOT NULL CHECK (venue ~ '^[a-z]+-[a-z]+$'),
  -- Epoch milliseconds: a minute for a recorded row, five for a backfilled one.
  time   BIGINT           NOT NULL,
  oi     DOUBLE PRECISION NOT NULL CHECK (oi >= 0),
  PRIMARY KEY (coin, venue, time)
);
CREATE INDEX open_interest_time_idx ON open_interest (time);

-- The coins being recorded: the configured ones, and any a chart has asked for.
CREATE TABLE open_interest_coin (
  coin      TEXT        PRIMARY KEY CHECK (coin ~ '^[A-Z0-9]{1,20}$'),
  added_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
