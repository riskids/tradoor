-- Training: paper trading on Binance's BTC, ETH and SOL perpetuals at their
-- live prices, for an account. One position a coin, its margin its own; a
-- reset starts a new run at 100,000 and keeps the runs before it to look back on.
CREATE TABLE training_account (
    owner      TEXT PRIMARY KEY,
    run        INTEGER NOT NULL DEFAULT 1,
    -- Cash: 100,000 to start, then every close, fee and funding.
    wallet     DOUBLE PRECISION NOT NULL,
    started_at BIGINT NOT NULL,
    -- Each coin's leverage, by symbol.
    leverage   JSONB NOT NULL DEFAULT '{}'
);

-- An open position: the coins held, at what average entry, the margin put up,
-- and what it has made and paid so far — its partial closes, fees and funding.
CREATE TABLE training_position (
    owner      TEXT NOT NULL,
    symbol     TEXT NOT NULL,
    side       TEXT NOT NULL,
    qty        DOUBLE PRECISION NOT NULL,
    entry      DOUBLE PRECISION NOT NULL,
    leverage   INTEGER NOT NULL,
    margin     DOUBLE PRECISION NOT NULL,
    tp         DOUBLE PRECISION,
    sl         DOUBLE PRECISION,
    opened_at  BIGINT NOT NULL,
    -- The most held at once, and the coins closed so far and what they fetched.
    peak       DOUBLE PRECISION NOT NULL,
    closed_qty DOUBLE PRECISION NOT NULL DEFAULT 0,
    closed_value DOUBLE PRECISION NOT NULL DEFAULT 0,
    realized   DOUBLE PRECISION NOT NULL DEFAULT 0,
    fees       DOUBLE PRECISION NOT NULL DEFAULT 0,
    funding    DOUBLE PRECISION NOT NULL DEFAULT 0,
    PRIMARY KEY (owner, symbol)
);

-- A limit order waiting for its price, with the take profit and stop loss the
-- position it opens takes.
CREATE TABLE training_order (
    id         BIGSERIAL PRIMARY KEY,
    owner      TEXT NOT NULL,
    symbol     TEXT NOT NULL,
    side       TEXT NOT NULL,
    price      DOUBLE PRECISION NOT NULL,
    qty        DOUBLE PRECISION NOT NULL,
    tp         DOUBLE PRECISION,
    sl         DOUBLE PRECISION,
    created_at BIGINT NOT NULL
);
CREATE INDEX training_order_owner_idx ON training_order (owner);

-- A position from its first fill to flat: the record the statistics are read from.
CREATE TABLE training_trade (
    id        BIGSERIAL PRIMARY KEY,
    owner     TEXT NOT NULL,
    run       INTEGER NOT NULL,
    symbol    TEXT NOT NULL,
    side      TEXT NOT NULL,
    qty       DOUBLE PRECISION NOT NULL,
    entry     DOUBLE PRECISION NOT NULL,
    exit      DOUBLE PRECISION NOT NULL,
    leverage  INTEGER NOT NULL,
    margin    DOUBLE PRECISION NOT NULL,
    pnl       DOUBLE PRECISION NOT NULL,
    fees      DOUBLE PRECISION NOT NULL,
    funding   DOUBLE PRECISION NOT NULL,
    -- How it ended: tp, sl, close, liquidation, or reversed by an order the other way.
    reason    TEXT NOT NULL,
    tp        DOUBLE PRECISION,
    sl        DOUBLE PRECISION,
    opened_at BIGINT NOT NULL,
    closed_at BIGINT NOT NULL
);
CREATE INDEX training_trade_owner_idx ON training_trade (owner, run, closed_at DESC);

-- Drawings on the training chart are its own.
ALTER TABLE drawing DROP CONSTRAINT drawing_chart_check;
ALTER TABLE drawing ADD CONSTRAINT drawing_chart_check CHECK (chart IN ('candles', 'tpo', 'training'));
