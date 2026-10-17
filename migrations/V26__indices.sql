-- Series this server works out itself, under the `index` venue.
--
-- What CoinGecko says the crypto market is worth, read every minute: the
-- whole, and in dollars the parts TradingView's CRYPTOCAP series take out of
-- it — Bitcoin, Ether, Tether, the ten largest together, and the stablecoins
-- (read every ten minutes, carried until the next). `at` is CoinGecko's own
-- time for the figures, so a read it had not updated is not kept twice.
CREATE TABLE market_cap (
    at     BIGINT PRIMARY KEY,
    total  DOUBLE PRECISION NOT NULL,
    btc    DOUBLE PRECISION NOT NULL,
    eth    DOUBLE PRECISION NOT NULL,
    usdt   DOUBLE PRECISION NOT NULL,
    top10  DOUBLE PRECISION NOT NULL,
    stable DOUBLE PRECISION
);

-- Daily bars of an index from its publisher: the VIX from Cboe. `final` once
-- the publisher's history carries the day; until then the day's bar is the
-- latest quote's, and a later quote may move it.
CREATE TABLE index_bar (
    symbol    TEXT NOT NULL,
    open_time BIGINT NOT NULL,
    open      DOUBLE PRECISION NOT NULL,
    high      DOUBLE PRECISION NOT NULL,
    low       DOUBLE PRECISION NOT NULL,
    close     DOUBLE PRECISION NOT NULL,
    final     BOOLEAN NOT NULL,
    PRIMARY KEY (symbol, open_time)
);
