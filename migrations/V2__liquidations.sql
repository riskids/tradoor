-- Liquidations of the few tickers the server follows, kept for a day.
--
-- A ticker is one coin and quote across exchanges (BTCUSDT on Binance and on
-- Bybit); `symbol` is how that exchange spells it. Rows are kept raw — a handful
-- of tickers is at most a few hundred thousand rows on a crash day — and the
-- server deletes anything older than 24 hours every minute.
CREATE TABLE liquidation (
  id      BIGSERIAL        PRIMARY KEY,
  ticker  TEXT             NOT NULL CHECK (ticker ~ '^[A-Z0-9]{2,30}$'),
  venue   TEXT             NOT NULL CHECK (venue ~ '^[a-z]+-perp$'),
  symbol  TEXT             NOT NULL,
  -- Epoch milliseconds, as the exchange stamped the fill.
  time    BIGINT           NOT NULL,
  price   DOUBLE PRECISION NOT NULL CHECK (price > 0),
  qty     DOUBLE PRECISION NOT NULL CHECK (qty > 0),
  usd     DOUBLE PRECISION NOT NULL CHECK (usd > 0),
  -- A long position was closed out (the exchange sold); false for a short.
  long    BOOLEAN          NOT NULL
);
CREATE INDEX liquidation_ticker_time_idx ON liquidation (ticker, time DESC);
CREATE INDEX liquidation_time_idx ON liquidation (time);
