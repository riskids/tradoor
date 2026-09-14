-- tradoor schema v1.
--
-- Postgres holds what is small, mutable and relational. Candle history is NOT
-- here: it lives in immutable columnar partition files (see docs/format.md),
-- and the `partition` table below is only their index.

-- Hand-drawn markup, per chart. A chart is a market (`venue:SYMBOL`, e.g.
-- `binance-perp:BTCUSDT`) at a timeframe; the same anchors on another timeframe
-- are a line through empty space, so drawings are scoped by both.
CREATE TABLE drawing (
  id          BIGSERIAL PRIMARY KEY,
  market      TEXT        NOT NULL CHECK (market ~ '^[a-z]+-(spot|perp):[A-Z0-9_]+$'),
  timeframe   TEXT        NOT NULL CHECK (timeframe ~ '^[0-9]{1,2}[mhdwM]$'),
  kind        TEXT        NOT NULL,
  -- [{ "time": <epoch ms>, "price": <number> }, ...] — validated by the server.
  points      JSONB       NOT NULL CHECK (jsonb_typeof(points) = 'array' AND jsonb_array_length(points) > 0),
  style       JSONB       NOT NULL DEFAULT '{}',
  label       TEXT,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX drawing_chart_idx ON drawing (market, timeframe, id);

-- The terminal layout: panels, their settings, theme and hotkeys, as one
-- document the UI reads and writes whole. `version` is bumped on every write so
-- two open tabs cannot silently overwrite each other.
CREATE TABLE workspace (
  id          TEXT        PRIMARY KEY,
  name        TEXT        NOT NULL,
  doc         JSONB       NOT NULL,
  version     INTEGER     NOT NULL DEFAULT 1,
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Markets whose history is kept on disk.
CREATE TABLE tracked_market (
  market      TEXT        PRIMARY KEY CHECK (market ~ '^[a-z]+-(spot|perp):[A-Z0-9_]+$'),
  history_from DATE       NOT NULL,
  enabled     BOOLEAN     NOT NULL DEFAULT TRUE,
  created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index of partition files. One row per file on disk.
CREATE TABLE partition (
  market      TEXT        NOT NULL,
  base        TEXT        NOT NULL CHECK (base IN ('1m', '1h', '1d')),
  -- '2026', '2026-08' or '2026-08-13': a closed year, month or day.
  period      TEXT        NOT NULL CHECK (period ~ '^[0-9]{4}(-[0-9]{2}(-[0-9]{2})?)?$'),
  rows        INTEGER     NOT NULL,
  bytes       INTEGER     NOT NULL,
  sha256      TEXT        NOT NULL,
  first_open  BIGINT      NOT NULL,
  last_open   BIGINT      NOT NULL,
  source      TEXT        NOT NULL,
  written_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (market, base, period)
);

-- Stretches the recorder could not fill. Written rather than hidden, so a hole
-- in the chart has an explanation next to it.
CREATE TABLE data_gap (
  market      TEXT        NOT NULL,
  base        TEXT        NOT NULL,
  from_open   BIGINT      NOT NULL,
  to_open     BIGINT      NOT NULL,
  reason      TEXT        NOT NULL,
  detected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (market, base, from_open)
);

-- Every backfill and recorder run, so an ingest that stops is visible.
CREATE TABLE ingest_run (
  id          BIGSERIAL   PRIMARY KEY,
  job         TEXT        NOT NULL,
  market      TEXT,
  status      TEXT        NOT NULL CHECK (status IN ('running', 'success', 'failed')),
  started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
  finished_at TIMESTAMPTZ,
  stats       JSONB       NOT NULL DEFAULT '{}',
  error       TEXT
);
CREATE INDEX ingest_run_recent_idx ON ingest_run (job, started_at DESC);

CREATE OR REPLACE FUNCTION touch_updated_at() RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at = now();
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER drawing_touch BEFORE UPDATE ON drawing FOR EACH ROW EXECUTE FUNCTION touch_updated_at();
