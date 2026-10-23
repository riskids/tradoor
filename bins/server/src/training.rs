//! Training: paper trading on Binance's BTC, ETH and SOL perpetuals at their
//! live prices, for an account (a browser signed in with Telegram).
//!
//! Each account starts with 100,000 USDT and one position a coin, Binance's
//! one-way mode: an order the position's way adds to it, the other way takes
//! from it, closes it, or turns it around. Margin is isolated: a position puts
//! up its notional over its leverage, and is liquidated when what is left of
//! that reaches Binance's maintenance margin — and then all of it is gone. A
//! fill pays Binance's fee, a maker's for a resting limit and a taker's for the
//! rest; a position held over a funding time pays or takes the funding.
//!
//! The prices come from Binance's own streams, followed here (`start`): the
//! trades fill market orders, limit orders and a position's take profit and
//! stop loss; the mark price liquidates and settles funding. So a training
//! account trades on with no browser open. A reset starts a new run at
//! 100,000, its open positions and orders gone; the runs before it stay, to
//! look back on.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::bail;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::Notify;
use tokio::time::{sleep, sleep_until, Instant};
use tokio_tungstenite::tungstenite::Message;
use tradoor_store::now_ms;
use tradoor_venues::Venue;

use crate::api::{owner, ApiError, ApiResult, AppState};
use crate::dns::Doh;
use crate::trades;

/// The coins trained on: Binance's perpetuals, by Binance's own names.
pub const SYMBOLS: [&str; 3] = ["BTCUSDT", "ETHUSDT", "SOLUSDT"];
const START: f64 = 100_000.0;
const MAKER: f64 = 0.0002;
const TAKER: f64 = 0.0005;
const MAX_LEVERAGE: i64 = 100;
const DEFAULT_LEVERAGE: i64 = 20;
/// Binance turns down an order worth less than this.
const MIN_NOTIONAL: f64 = 5.0;
const MAX_ORDERS: i64 = 50;
/// Closed trades one run lists.
const MAX_TRADES: i64 = 5_000;
/// Coins under this are none: what float arithmetic leaves of a whole close.
const DUST: f64 = 1e-9;

const RETRY_SECS: [u64; 5] = [1, 3, 5, 10, 30];
/// Nothing at all from Binance for this long means the socket is gone.
const SILENCE: Duration = Duration::from_secs(60);
/// The open orders and positions are read again this often, besides whenever the API changes them.
const RELOAD: Duration = Duration::from_secs(5);

/// Binance's maintenance margin rate for a position's first tier.
fn mmr(symbol: &str) -> f64 {
    if symbol == "BTCUSDT" { 0.004 } else { 0.005 }
}

fn known(symbol: &str) -> bool {
    SYMBOLS.contains(&symbol)
}

// ---------------------------------------------------------------- prices

/// A coin's prices as the stream last brought them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quote {
    pub last: f64,
    pub mark: f64,
    /// The funding rate the next settlement takes, and when that is.
    pub funding: f64,
    pub next_funding: i64,
    pub at: i64,
}

/// The live side: the coins' prices, and a nudge for the engine when the API
/// changes the orders and positions it watches.
#[derive(Default)]
pub struct Live {
    quotes: RwLock<HashMap<String, Quote>>,
    changed: Notify,
}

impl Live {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn quote(&self, symbol: &str) -> Option<Quote> {
        self.quotes.read().ok()?.get(symbol).copied().filter(|q| q.last > 0.0)
    }

    fn quotes(&self) -> HashMap<String, Quote> {
        self.quotes.read().map(|q| q.clone()).unwrap_or_default()
    }

    fn update(&self, symbol: &str, f: impl FnOnce(&mut Quote)) {
        if let Ok(mut quotes) = self.quotes.write() {
            f(quotes.entry(symbol.to_owned()).or_default());
        }
    }
}

// ---------------------------------------------------------------- positions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Long,
    Short,
}

impl Side {
    fn of(s: &str) -> Option<Self> {
        match s {
            "long" => Some(Self::Long),
            "short" => Some(Self::Short),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Long => "long",
            Self::Short => "short",
        }
    }

    fn dir(self) -> f64 {
        match self {
            Self::Long => 1.0,
            Self::Short => -1.0,
        }
    }

    /// The side a buy (or a sell) opens.
    fn of_order(buy: bool) -> Self {
        if buy { Self::Long } else { Self::Short }
    }
}

/// An open position, and what it has done so far.
#[derive(Clone, Debug, PartialEq)]
pub struct Position {
    pub side: Side,
    pub qty: f64,
    pub entry: f64,
    pub leverage: i64,
    pub margin: f64,
    pub tp: Option<f64>,
    pub sl: Option<f64>,
    pub opened_at: i64,
    /// The most held at once.
    pub peak: f64,
    pub closed_qty: f64,
    pub closed_value: f64,
    pub realized: f64,
    pub fees: f64,
    pub funding: f64,
}

impl Position {
    /// The price at which what is left of the margin is the maintenance
    /// margin Binance keeps: `margin + (P − entry)·qty = P·qty·mmr` for a long.
    pub fn liquidation(&self, mmr: f64) -> f64 {
        let at = match self.side {
            Side::Long => (self.entry * self.qty - self.margin) / (self.qty * (1.0 - mmr)),
            Side::Short => (self.entry * self.qty + self.margin) / (self.qty * (1.0 + mmr)),
        };
        at.max(0.0)
    }
}

/// A position from its first fill to flat.
#[derive(Clone, Debug, PartialEq)]
pub struct Closed {
    pub side: Side,
    pub qty: f64,
    pub entry: f64,
    pub exit: f64,
    pub leverage: i64,
    pub margin: f64,
    pub pnl: f64,
    pub fees: f64,
    pub funding: f64,
    pub tp: Option<f64>,
    pub sl: Option<f64>,
    pub opened_at: i64,
}

/// What a fill leaves: the position, the trade it closed if it closed one, and
/// the cash it moved — what was closed made, less the fee.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub position: Option<Position>,
    pub closed: Option<Closed>,
    pub cash: f64,
}

/// One fill of `qty` coins at `price` against the position there is, one-way:
/// the position's way it adds, at the average entry; the other way it takes
/// off, closes, and what is left over opens the other way. A position opened
/// takes `tp` and `sl`; one added to keeps its own unless they are given.
#[allow(clippy::too_many_arguments)]
pub fn fill(pos: Option<Position>, buy: bool, qty: f64, price: f64, fee_rate: f64, leverage: i64, tp: Option<f64>, sl: Option<f64>, now: i64) -> Outcome {
    let side = Side::of_order(buy);
    let fee_of = |q: f64| q * price * fee_rate;
    let open = |q: f64| Position {
        side,
        qty: q,
        entry: price,
        leverage,
        margin: q * price / leverage as f64,
        tp,
        sl,
        opened_at: now,
        peak: q,
        closed_qty: 0.0,
        closed_value: 0.0,
        realized: 0.0,
        fees: fee_of(q),
        funding: 0.0,
    };
    match pos {
        None => Outcome { position: Some(open(qty)), closed: None, cash: -fee_of(qty) },
        Some(mut p) if p.side == side => {
            let held = p.qty + qty;
            p.entry = (p.entry * p.qty + price * qty) / held;
            p.margin += qty * price / p.leverage as f64;
            p.qty = held;
            p.peak = p.peak.max(held);
            p.fees += fee_of(qty);
            p.tp = tp.or(p.tp);
            p.sl = sl.or(p.sl);
            Outcome { position: Some(p), closed: None, cash: -fee_of(qty) }
        }
        Some(mut p) => {
            let off = qty.min(p.qty);
            let pnl = (price - p.entry) * p.side.dir() * off;
            let fee = fee_of(off);
            p.realized += pnl;
            p.fees += fee;
            p.closed_qty += off;
            p.closed_value += off * price;
            p.margin -= p.margin * off / p.qty;
            p.qty -= off;
            let cash = pnl - fee;
            if p.qty > DUST {
                return Outcome { position: Some(p), closed: None, cash };
            }
            let closed = Closed {
                side: p.side,
                qty: p.closed_qty,
                entry: p.entry,
                exit: p.closed_value / p.closed_qty,
                leverage: p.leverage,
                margin: p.entry * p.peak / p.leverage as f64,
                pnl: p.realized,
                fees: p.fees,
                funding: p.funding,
                tp: p.tp,
                sl: p.sl,
                opened_at: p.opened_at,
            };
            let rest = qty - off;
            if rest > DUST {
                Outcome { position: Some(open(rest)), closed: Some(closed), cash: cash - fee_of(rest) }
            } else {
                Outcome { position: None, closed: Some(closed), cash }
            }
        }
    }
}

/// What a position held over a funding time takes (negative: pays): a long
/// pays a positive rate on its notional at the mark, a short takes it.
pub fn funding_of(side: Side, qty: f64, mark: f64, rate: f64) -> f64 {
    -side.dir() * qty * mark * rate
}

// ---------------------------------------------------------------- what the engine watches

#[derive(Clone, Debug, PartialEq)]
struct WatchedOrder {
    id: i64,
    symbol: String,
    buy: bool,
    price: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct WatchedPosition {
    owner: String,
    symbol: String,
    side: Side,
    tp: Option<f64>,
    sl: Option<f64>,
    liquidation: f64,
}

/// Something the price has reached.
#[derive(Clone, Debug, PartialEq)]
enum Trigger {
    Order(i64),
    Exit { owner: String, symbol: String, price: f64, reason: &'static str },
}

/// The open orders and positions, as last read.
#[derive(Clone, Debug, Default)]
struct Watch {
    orders: Vec<WatchedOrder>,
    positions: Vec<WatchedPosition>,
}

impl Watch {
    async fn load(pool: &deadpool_postgres::Pool) -> anyhow::Result<Self> {
        let client = pool.get().await?;
        let orders = client
            .query("SELECT id, symbol, side, price FROM training_order", &[])
            .await?
            .iter()
            .map(|r| WatchedOrder { id: r.get(0), symbol: r.get(1), buy: r.get::<_, String>(2) == "buy", price: r.get(3) })
            .collect();
        let positions = client
            .query("SELECT owner, symbol, side, qty, entry, margin, tp, sl FROM training_position", &[])
            .await?
            .iter()
            .filter_map(|r| {
                let symbol: String = r.get(1);
                let side = Side::of(r.get(2))?;
                let (qty, entry, margin): (f64, f64, f64) = (r.get(3), r.get(4), r.get(5));
                let liquidation = Position { side, qty, entry, margin, leverage: 1, tp: None, sl: None, opened_at: 0, peak: qty, closed_qty: 0.0, closed_value: 0.0, realized: 0.0, fees: 0.0, funding: 0.0 }
                    .liquidation(mmr(&symbol));
                Some(WatchedPosition { owner: r.get(0), symbol, side, tp: r.get(6), sl: r.get(7), liquidation })
            })
            .collect();
        Ok(Self { orders, positions })
    }

    /// What a trade at `price` reaches: limit orders it crosses, and take
    /// profits and stop losses — a stop first, should one price reach both.
    fn due(&self, symbol: &str, price: f64) -> Vec<Trigger> {
        let orders = self.orders.iter().filter(|o| o.symbol == symbol && if o.buy { price <= o.price } else { price >= o.price }).map(|o| Trigger::Order(o.id));
        let exits = self.positions.iter().filter(|p| p.symbol == symbol).filter_map(|p| {
            let dir = p.side.dir();
            let (reason, at) = if p.sl.is_some_and(|sl| (price - sl) * dir <= 0.0) {
                ("sl", p.sl?)
            } else if p.tp.is_some_and(|tp| (price - tp) * dir >= 0.0) {
                ("tp", p.tp?)
            } else {
                return None;
            };
            Some(Trigger::Exit { owner: p.owner.clone(), symbol: p.symbol.clone(), price: at, reason })
        });
        orders.chain(exits).collect()
    }

    /// Positions the mark has carried to their liquidation price.
    fn liquidations(&self, symbol: &str, mark: f64) -> Vec<Trigger> {
        self.positions
            .iter()
            .filter(|p| p.symbol == symbol && (mark - p.liquidation) * p.side.dir() <= 0.0)
            .map(|p| Trigger::Exit { owner: p.owner.clone(), symbol: p.symbol.clone(), price: p.liquidation, reason: "liquidation" })
            .collect()
    }
}

// ---------------------------------------------------------------- the database

/// An account's run, cash and leverages, locked for the transaction: the
/// engine and the API never move one account at the same time.
struct Account {
    run: i32,
    wallet: f64,
    leverage: Map<String, Value>,
}

impl Account {
    fn leverage_of(&self, symbol: &str) -> i64 {
        self.leverage.get(symbol).and_then(Value::as_i64).map_or(DEFAULT_LEVERAGE, |l| l.clamp(1, MAX_LEVERAGE))
    }
}

async fn ensure(client: &tokio_postgres::Client, owner: &str) -> Result<(), tokio_postgres::Error> {
    client
        .execute("INSERT INTO training_account (owner, wallet, started_at) VALUES ($1, $2, $3) ON CONFLICT (owner) DO NOTHING", &[&owner, &START, &now_ms()])
        .await?;
    Ok(())
}

async fn lock(tx: &tokio_postgres::Transaction<'_>, owner: &str) -> Result<Option<Account>, tokio_postgres::Error> {
    Ok(tx.query_opt("SELECT run, wallet, leverage FROM training_account WHERE owner = $1 FOR UPDATE", &[&owner]).await?.map(|r| Account {
        run: r.get(0),
        wallet: r.get(1),
        leverage: r.get::<_, Value>(2).as_object().cloned().unwrap_or_default(),
    }))
}

const POSITION_COLUMNS: &str = "side, qty, entry, leverage, margin, tp, sl, opened_at, peak, closed_qty, closed_value, realized, fees, funding";

fn position_of(r: &tokio_postgres::Row) -> Option<Position> {
    Some(Position {
        side: Side::of(r.get(0))?,
        qty: r.get(1),
        entry: r.get(2),
        leverage: i64::from(r.get::<_, i32>(3)),
        margin: r.get(4),
        tp: r.get(5),
        sl: r.get(6),
        opened_at: r.get(7),
        peak: r.get(8),
        closed_qty: r.get(9),
        closed_value: r.get(10),
        realized: r.get(11),
        fees: r.get(12),
        funding: r.get(13),
    })
}

async fn position(tx: &tokio_postgres::Transaction<'_>, owner: &str, symbol: &str) -> Result<Option<Position>, tokio_postgres::Error> {
    Ok(tx
        .query_opt(&format!("SELECT {POSITION_COLUMNS} FROM training_position WHERE owner = $1 AND symbol = $2 FOR UPDATE"), &[&owner, &symbol])
        .await?
        .as_ref()
        .and_then(position_of))
}

async fn save(tx: &tokio_postgres::Transaction<'_>, owner: &str, symbol: &str, pos: Option<&Position>) -> Result<(), tokio_postgres::Error> {
    let Some(p) = pos else {
        tx.execute("DELETE FROM training_position WHERE owner = $1 AND symbol = $2", &[&owner, &symbol]).await?;
        return Ok(());
    };
    let leverage = p.leverage as i32;
    tx.execute(
        &format!(
            "INSERT INTO training_position (owner, symbol, {POSITION_COLUMNS}) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
             ON CONFLICT (owner, symbol) DO UPDATE SET side = $3, qty = $4, entry = $5, leverage = $6, margin = $7, tp = $8, sl = $9, opened_at = $10,
                 peak = $11, closed_qty = $12, closed_value = $13, realized = $14, fees = $15, funding = $16"
        ),
        &[&owner, &symbol, &p.side.id(), &p.qty, &p.entry, &leverage, &p.margin, &p.tp, &p.sl, &p.opened_at, &p.peak, &p.closed_qty, &p.closed_value, &p.realized, &p.fees, &p.funding],
    )
    .await?;
    Ok(())
}

async fn record(tx: &tokio_postgres::Transaction<'_>, owner: &str, run: i32, symbol: &str, c: &Closed, reason: &str, now: i64) -> Result<(), tokio_postgres::Error> {
    let leverage = c.leverage as i32;
    tx.execute(
        "INSERT INTO training_trade (owner, run, symbol, side, qty, entry, exit, leverage, margin, pnl, fees, funding, reason, tp, sl, opened_at, closed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        &[&owner, &run, &symbol, &c.side.id(), &c.qty, &c.entry, &c.exit, &leverage, &c.margin, &c.pnl, &c.fees, &c.funding, &reason, &c.tp, &c.sl, &c.opened_at, &now],
    )
    .await?;
    Ok(())
}

/// What an account has free to put up: its cash, less the margin its
/// positions hold and its limit orders would take — an order the other way
/// only for what it would open past the position it closes.
async fn available(tx: &tokio_postgres::Transaction<'_>, owner: &str, account: &Account) -> Result<f64, tokio_postgres::Error> {
    let positions = tx.query("SELECT symbol, side, qty, margin FROM training_position WHERE owner = $1", &[&owner]).await?;
    let held: HashMap<String, (String, f64)> = positions.iter().map(|r| (r.get::<_, String>(0), (r.get::<_, String>(1), r.get::<_, f64>(2)))).collect();
    let mut used: f64 = positions.iter().map(|r| r.get::<_, f64>(3)).sum();
    for r in tx.query("SELECT symbol, side, price, qty FROM training_order WHERE owner = $1", &[&owner]).await? {
        let (symbol, side, price, qty): (String, String, f64, f64) = (r.get(0), r.get(1), r.get(2), r.get(3));
        let against = held.get(&symbol).filter(|(s, _)| (s == "long") != (side == "buy")).map_or(0.0, |(_, q)| *q);
        used += (qty - against).max(0.0) * price / account.leverage_of(&symbol) as f64;
    }
    Ok(account.wallet - used)
}

/// One fill for an account, in one transaction: the position moved, the cash
/// moved, and a trade written if it closed one. `reason` names how a close
/// came about; `extra_fee` is what a liquidation takes besides.
#[allow(clippy::too_many_arguments)]
async fn execute(
    tx: &tokio_postgres::Transaction<'_>,
    owner: &str,
    account: &Account,
    symbol: &str,
    buy: bool,
    qty: f64,
    price: f64,
    fee_rate: f64,
    tp: Option<f64>,
    sl: Option<f64>,
    reason: &str,
) -> Result<Outcome, tokio_postgres::Error> {
    let now = now_ms();
    let before = position(tx, owner, symbol).await?;
    let leverage = before.as_ref().map_or_else(|| account.leverage_of(symbol), |p| p.leverage);
    // Liquidated, a position loses its whole margin: what the close at the
    // liquidation price leaves of it goes too, as Binance's clearance fee.
    let clearance = before.as_ref().filter(|_| reason == "liquidation").map_or(0.0, |p| (p.margin + (price - p.entry) * p.side.dir() * p.qty).max(0.0));
    let mut outcome = fill(before, buy, qty, price, fee_rate, leverage, tp, sl, now);
    if let Some(c) = outcome.closed.as_mut() {
        c.fees += clearance;
        outcome.cash -= clearance;
    }
    save(tx, owner, symbol, outcome.position.as_ref()).await?;
    if let Some(c) = &outcome.closed {
        let reason = if outcome.position.is_some() { "reversed" } else { reason };
        record(tx, owner, account.run, symbol, c, reason, now).await?;
    }
    tx.execute("UPDATE training_account SET wallet = wallet + $2 WHERE owner = $1", &[&owner, &outcome.cash]).await?;
    Ok(outcome)
}

// ---------------------------------------------------------------- the engine

/// Follow Binance's trades and mark prices for the three coins, for as long as
/// the server runs, filling what they reach.
pub fn start(pool: deadpool_postgres::Pool, doh: Arc<Doh>, live: Arc<Live>) {
    tokio::spawn(async move {
        let urls = trades::stream_urls(Venue::BinancePerp);
        let (mut attempt, mut at) = (0, 0);
        loop {
            let url = urls[at % urls.len()];
            match session(url, &pool, &doh, &live).await {
                Ok(()) => attempt = 0,
                Err(e) => {
                    tracing::warn!(url, error = %e, "training stream lost");
                    sleep(Duration::from_secs(RETRY_SECS[attempt.min(RETRY_SECS.len() - 1)])).await;
                    (attempt, at) = (attempt + 1, at + 1);
                }
            }
        }
    });
}

/// A mark price update: `{"e":"markPriceUpdate","s":…,"p":mark,"r":rate,"T":next funding}`.
fn mark_update(text: &str) -> Option<(String, f64, f64, i64)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let v = v.get("data").unwrap_or(&v);
    (v["e"].as_str()? == "markPriceUpdate").then_some(())?;
    let num = |k: &str| v[k].as_str().and_then(|s| s.parse::<f64>().ok());
    Some((v["s"].as_str()?.to_owned(), num("p")?, num("r").unwrap_or(0.0), v["T"].as_i64().unwrap_or(0)))
}

async fn session(url: &str, pool: &deadpool_postgres::Pool, doh: &Doh, live: &Live) -> anyhow::Result<()> {
    let mut ws = tokio::time::timeout(Duration::from_secs(15), doh.connect_ws(url)).await.map_err(|_| anyhow::anyhow!("connecting timed out"))??;
    let symbols: Vec<String> = SYMBOLS.iter().map(|s| (*s).to_owned()).collect();
    for frame in trades::subscribe(Venue::BinancePerp, &symbols) {
        ws.send(Message::text(frame)).await?;
    }
    let marks: Vec<String> = SYMBOLS.iter().map(|s| format!("\"{}@markPrice@1s\"", s.to_lowercase())).collect();
    ws.send(Message::text(format!("{{\"method\":\"SUBSCRIBE\",\"params\":[{}],\"id\":2}}", marks.join(",")))).await?;
    tracing::info!(url, "training on Binance's live prices");

    let mut watch = Watch::load(pool).await?;
    let mut reload = tokio::time::interval(RELOAD);
    reload.reset();
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            frame = ws.next() => {
                let Some(frame) = frame else { return Ok(()) };
                heard = Instant::now();
                let text = match frame? {
                    Message::Text(text) => text.to_string(),
                    Message::Close(_) => return Ok(()),
                    _ => continue,
                };
                let now = now_ms();
                // A frame's prints reach their orders and exits against the same
                // reading; one filled twice finds itself gone the second time.
                let mut acted = false;
                for (symbol, price, _, _) in trades::prints(Venue::BinancePerp, &text) {
                    live.update(&symbol, |q| {
                        q.last = price;
                        q.at = now;
                    });
                    for trigger in watch.due(&symbol, price) {
                        act(pool, &trigger).await;
                        acted = true;
                    }
                }
                if acted {
                    watch = Watch::load(pool).await?;
                }
                if let Some((symbol, mark, rate, next)) = mark_update(&text) {
                    let before = live.quote(&symbol).unwrap_or_default();
                    live.update(&symbol, |q| {
                        q.mark = mark;
                        q.funding = rate;
                        q.next_funding = next;
                    });
                    // The funding time passed: settle at the rate it was for.
                    if before.next_funding > 0
                        && next > before.next_funding
                        && now >= before.next_funding
                        && let Err(e) = settle(pool, &symbol, before.funding, mark, before.next_funding).await
                    {
                        tracing::warn!(%symbol, error = %e, "settling training funding");
                    }
                    let due = watch.liquidations(&symbol, mark);
                    for trigger in &due {
                        act(pool, trigger).await;
                    }
                    if !due.is_empty() {
                        watch = Watch::load(pool).await?;
                    }
                }
            }
            () = live.changed.notified() => watch = Watch::load(pool).await?,
            _ = reload.tick() => watch = Watch::load(pool).await?,
            () = sleep_until(heard + SILENCE) => bail!("nothing heard for {} seconds", SILENCE.as_secs()),
        }
    }
}

/// Fill what the price reached, if it is still there to fill.
async fn act(pool: &deadpool_postgres::Pool, trigger: &Trigger) {
    let done = async {
        let mut client = pool.get().await?;
        let tx = client.transaction().await?;
        match trigger {
            Trigger::Order(id) => {
                let Some(r) = tx.query_opt("DELETE FROM training_order WHERE id = $1 RETURNING owner, symbol, side, price, qty, tp, sl", &[id]).await? else { return Ok(()) };
                let (owner, symbol, side, price, qty): (String, String, String, f64, f64) = (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4));
                let Some(account) = lock(&tx, &owner).await? else { return Ok(()) };
                execute(&tx, &owner, &account, &symbol, side == "buy", qty, price, MAKER, r.get(5), r.get(6), "close").await?;
            }
            Trigger::Exit { owner, symbol, price, reason } => {
                let Some(account) = lock(&tx, owner).await? else { return Ok(()) };
                let Some(p) = position(&tx, owner, symbol).await? else { return Ok(()) };
                // Still the level it was reached at: not moved, not closed meanwhile.
                let still = match *reason {
                    "tp" => p.tp == Some(*price),
                    "sl" => p.sl == Some(*price),
                    _ => (p.liquidation(mmr(symbol)) - price).abs() < 1e-9 * price.max(1.0),
                };
                if !still {
                    return Ok(());
                }
                let fee = if *reason == "liquidation" { 0.0 } else { TAKER };
                execute(&tx, owner, &account, symbol, p.side == Side::Short, p.qty, *price, fee, None, None, reason).await?;
            }
        }
        tx.commit().await?;
        tracing::debug!(?trigger, "training fill");
        anyhow::Ok(())
    };
    if let Err(e) = done.await {
        tracing::warn!(?trigger, error = %e, "training fill");
    }
}

/// A funding time passed: every position held over it pays or takes it.
async fn settle(pool: &deadpool_postgres::Pool, symbol: &str, rate: f64, mark: f64, at: i64) -> anyhow::Result<()> {
    let mut client = pool.get().await?;
    let owners: Vec<String> = client
        .query("SELECT owner FROM training_position WHERE symbol = $1 AND opened_at <= $2", &[&symbol, &at])
        .await?
        .iter()
        .map(|r| r.get(0))
        .collect();
    for owner in owners {
        let tx = client.transaction().await?;
        if lock(&tx, &owner).await?.is_some()
            && let Some(p) = position(&tx, &owner, symbol).await?.filter(|p| p.opened_at <= at)
        {
            let paid = funding_of(p.side, p.qty, mark, rate);
            tx.execute("UPDATE training_position SET funding = funding + $3 WHERE owner = $1 AND symbol = $2", &[&owner, &symbol, &paid]).await?;
            tx.execute("UPDATE training_account SET wallet = wallet + $2 WHERE owner = $1", &[&owner, &paid]).await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- the API

/// What a browser not signed in is told.
const SIGN_IN_FIRST: &str = "sign in with Telegram to train";

async fn account_only(client: &tokio_postgres::Client, owner: &str) -> ApiResult<()> {
    let ok: bool = client.query_one("SELECT EXISTS (SELECT 1 FROM account WHERE owner = $1)", &[&owner]).await?.get(0);
    if ok { Ok(()) } else { Err(ApiError::forbidden(SIGN_IN_FIRST)) }
}

fn symbol_of(raw: &str) -> ApiResult<String> {
    let s = raw.to_uppercase();
    if known(&s) { Ok(s) } else { Err(ApiError::bad_request(format!("training trades {} only", SYMBOLS.join(", ")))) }
}

/// A positive finite number from the body, or `None` when absent or null.
fn price_field(b: &Map<String, Value>, key: &str) -> ApiResult<Option<f64>> {
    match b.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|v| v.is_finite() && *v > 0.0).map(Some).ok_or_else(|| ApiError::bad_request(format!("{key} must be a positive number"))),
    }
}

/// A take profit and stop loss make sense for a position this way at `price`:
/// the profit past it, the stop short of it.
fn check_exits(side: Side, price: f64, tp: Option<f64>, sl: Option<f64>) -> ApiResult<()> {
    let dir = side.dir();
    if tp.is_some_and(|tp| (tp - price) * dir <= 0.0) {
        return Err(ApiError::bad_request(format!("a {}'s take profit is {} its entry", side.id(), if dir > 0.0 { "above" } else { "below" })));
    }
    if sl.is_some_and(|sl| (sl - price) * dir >= 0.0) {
        return Err(ApiError::bad_request(format!("a {}'s stop loss is {} its entry", side.id(), if dir > 0.0 { "below" } else { "above" })));
    }
    Ok(())
}

fn quotes_json(live: &Live) -> Value {
    let quotes = live.quotes();
    Value::Object(
        SYMBOLS
            .iter()
            .filter_map(|s| {
                let q = quotes.get(*s)?;
                Some(((*s).to_owned(), json!({ "last": q.last, "mark": q.mark, "funding": q.funding, "nextFunding": q.next_funding, "at": q.at })))
            })
            .collect(),
    )
}

/// `GET /api/training`: the account as it stands — cash, leverages, positions
/// with their liquidation prices, open orders — and the coins' live prices.
pub async fn state(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    ensure(&client, &owner).await?;
    let a = client.query_one("SELECT run, wallet, started_at, leverage FROM training_account WHERE owner = $1", &[&owner]).await?;
    let leverage = a.get::<_, Value>(3).as_object().cloned().unwrap_or_default();
    let leverage: Map<String, Value> = SYMBOLS
        .iter()
        .map(|s| ((*s).to_owned(), json!(leverage.get(*s).and_then(Value::as_i64).map_or(DEFAULT_LEVERAGE, |l| l.clamp(1, MAX_LEVERAGE)))))
        .collect();
    let positions: Vec<Value> = client
        .query(&format!("SELECT symbol, {POSITION_COLUMNS} FROM training_position WHERE owner = $1 ORDER BY symbol"), &[&owner])
        .await?
        .iter()
        .filter_map(|r| {
            let symbol: String = r.get(0);
            let p = Position {
                side: Side::of(r.get(1))?,
                qty: r.get(2),
                entry: r.get(3),
                leverage: i64::from(r.get::<_, i32>(4)),
                margin: r.get(5),
                tp: r.get(6),
                sl: r.get(7),
                opened_at: r.get(8),
                peak: r.get(9),
                closed_qty: r.get(10),
                closed_value: r.get(11),
                realized: r.get(12),
                fees: r.get(13),
                funding: r.get(14),
            };
            Some(json!({
                "symbol": symbol, "side": p.side.id(), "qty": p.qty, "entry": p.entry, "leverage": p.leverage, "margin": p.margin,
                "tp": p.tp, "sl": p.sl, "liquidation": p.liquidation(mmr(&symbol)), "openedAt": p.opened_at,
                "realized": p.realized, "fees": p.fees, "funding": p.funding,
            }))
        })
        .collect();
    let orders: Vec<Value> = client
        .query("SELECT id, symbol, side, price, qty, tp, sl, created_at FROM training_order WHERE owner = $1 ORDER BY created_at", &[&owner])
        .await?
        .iter()
        .map(|r| {
            json!({ "id": r.get::<_, i64>(0), "symbol": r.get::<_, String>(1), "side": r.get::<_, String>(2), "price": r.get::<_, f64>(3), "qty": r.get::<_, f64>(4),
                    "tp": r.get::<_, Option<f64>>(5), "sl": r.get::<_, Option<f64>>(6), "createdAt": r.get::<_, i64>(7) })
        })
        .collect();
    let mmrs: Map<String, Value> = SYMBOLS.iter().map(|s| ((*s).to_owned(), json!(mmr(s)))).collect();
    Ok(Json(json!({
        "run": a.get::<_, i32>(0), "wallet": a.get::<_, f64>(1), "startedAt": a.get::<_, i64>(2), "start": START,
        "leverage": leverage, "maxLeverage": MAX_LEVERAGE, "maker": MAKER, "taker": TAKER, "mmr": mmrs,
        "positions": positions, "orders": orders, "quotes": quotes_json(&state.training),
    })))
}

/// `POST /api/training/orders` `{symbol, side: buy|sell, type: market|limit, qty, price?, tp?, sl?}`:
/// a market order fills at the last trade, as a taker; a limit order rests
/// until a trade reaches it, filling as a maker — or, priced through the
/// market, fills at once as a market order would.
pub async fn order(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner = owner(&headers)?;
    let b = body.as_object().ok_or_else(|| ApiError::bad_request("body must be a JSON object"))?;
    let symbol = symbol_of(b.get("symbol").and_then(Value::as_str).unwrap_or(""))?;
    let buy = match b.get("side").and_then(Value::as_str) {
        Some("buy") => true,
        Some("sell") => false,
        _ => return Err(ApiError::bad_request("side is buy or sell")),
    };
    let limit = match b.get("type").and_then(Value::as_str) {
        Some("market") => false,
        Some("limit") => true,
        _ => return Err(ApiError::bad_request("type is market or limit")),
    };
    let qty = price_field(b, "qty")?.ok_or_else(|| ApiError::bad_request("qty must be a positive number"))?;
    let (tp, sl) = (price_field(b, "tp")?, price_field(b, "sl")?);
    let last = state.training.quote(&symbol).map(|q| q.last).ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "no live price yet — try again in a moment".into()))?;
    let price = if limit { price_field(b, "price")?.ok_or_else(|| ApiError::bad_request("a limit order needs its price"))? } else { last };
    // Priced through the market, a limit takes what is there now.
    let resting = limit && if buy { price < last } else { price > last };
    let fill_at = if resting { price } else { last };
    if qty * fill_at < MIN_NOTIONAL {
        return Err(ApiError::bad_request(format!("an order is worth at least {MIN_NOTIONAL} USDT")));
    }

    let mut client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    ensure(&client, &owner).await?;
    let tx = client.transaction().await?;
    let account = lock(&tx, &owner).await?.ok_or_else(|| ApiError::not_found("no training account"))?;
    let held = position(&tx, &owner, &symbol).await?;
    let against = held.as_ref().filter(|p| p.side != Side::of_order(buy)).map_or(0.0, |p| p.qty);
    let opens = (qty - against).max(0.0);
    let leverage = held.as_ref().map_or_else(|| account.leverage_of(&symbol), |p| p.leverage);
    // A take profit and stop loss go with what the order opens or adds to, the
    // stop short of where that position would be liquidated.
    if opens > 0.0 {
        check_exits(Side::of_order(buy), fill_at, tp, sl)?;
        let after = fill(held.clone(), buy, qty, fill_at, 0.0, leverage, tp, sl, 0).position;
        if let (Some(sl), Some(p)) = (sl, after) {
            let liquidation = p.liquidation(mmr(&symbol));
            if (sl - liquidation) * p.side.dir() <= 0.0 {
                return Err(ApiError::bad_request(format!("the stop loss is past the liquidation price, {liquidation:.2}: lower the leverage or bring the stop closer")));
            }
        }
    }
    let need = opens * fill_at / leverage as f64 + qty * fill_at * if resting { 0.0 } else { TAKER };
    let free = available(&tx, &owner, &account).await?;
    if need > free + 1e-6 {
        return Err(ApiError::bad_request(format!("not enough margin: this takes {need:.2} USDT, {free:.2} is free")));
    }
    let answer = if resting {
        let count: i64 = tx.query_one("SELECT count(*) FROM training_order WHERE owner = $1", &[&owner]).await?.get(0);
        if count >= MAX_ORDERS {
            return Err(ApiError::bad_request(format!("at most {MAX_ORDERS} open orders")));
        }
        let id: i64 = tx
            .query_one(
                "INSERT INTO training_order (owner, symbol, side, price, qty, tp, sl, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
                &[&owner, &symbol, &if buy { "buy" } else { "sell" }, &price, &qty, &tp, &sl, &now_ms()],
            )
            .await?
            .get(0);
        json!({ "order": id })
    } else {
        let outcome = execute(&tx, &owner, &account, &symbol, buy, qty, fill_at, TAKER, tp, sl, "close").await?;
        json!({ "filled": { "price": fill_at, "qty": qty }, "closed": outcome.closed.is_some() })
    };
    tx.commit().await?;
    state.training.changed.notify_one();
    Ok((StatusCode::CREATED, Json(answer)))
}

/// `DELETE /api/training/orders/{id}`: a limit order cancelled.
pub async fn cancel(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    if client.execute("DELETE FROM training_order WHERE id = $1 AND owner = $2", &[&id, &owner]).await? == 0 {
        return Err(ApiError::not_found(format!("order {id} not found")));
    }
    state.training.changed.notify_one();
    Ok(Json(json!({ "id": id })))
}

/// `POST /api/training/positions/{symbol}/close` `{qty?}`: closed at the last
/// trade, all of it or `qty` of it, as a taker.
pub async fn close(State(state): State<AppState>, headers: HeaderMap, Path(symbol): Path<String>, body: Option<Json<Value>>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let symbol = symbol_of(&symbol)?;
    let body = body.map_or(Value::Null, |Json(b)| b);
    let part = body.as_object().map(|b| price_field(b, "qty")).transpose()?.flatten();
    let last = state.training.quote(&symbol).map(|q| q.last).ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "no live price yet — try again in a moment".into()))?;
    let mut client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    let tx = client.transaction().await?;
    let account = lock(&tx, &owner).await?.ok_or_else(|| ApiError::not_found("no training account"))?;
    let p = position(&tx, &owner, &symbol).await?.ok_or_else(|| ApiError::not_found(format!("no {symbol} position")))?;
    let qty = part.map_or(p.qty, |q| q.min(p.qty));
    let outcome = execute(&tx, &owner, &account, &symbol, p.side == Side::Short, qty, last, TAKER, None, None, "close").await?;
    tx.commit().await?;
    state.training.changed.notify_one();
    Ok(Json(json!({ "price": last, "qty": qty, "closed": outcome.closed.is_some() })))
}

/// What turning a position around at `price` puts up — the margin of as much
/// the other way, and the taker fee on both — and what closing it gives back
/// first: its margin, with what it made.
fn turn_cost(p: &Position, price: f64) -> (f64, f64) {
    let need = p.qty * price / p.leverage as f64 + 2.0 * p.qty * price * TAKER;
    let freed = p.margin + (price - p.entry) * p.side.dir() * p.qty;
    (need, freed)
}

/// `POST /api/training/positions/{symbol}/reverse`: the position turned the
/// other way at the last trade, as a taker, in one fill — closed, and as much
/// opened against it at its leverage, with no take profit or stop loss.
pub async fn reverse(State(state): State<AppState>, headers: HeaderMap, Path(symbol): Path<String>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let symbol = symbol_of(&symbol)?;
    let last = state.training.quote(&symbol).map(|q| q.last).ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "no live price yet — try again in a moment".into()))?;
    let mut client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    let tx = client.transaction().await?;
    let account = lock(&tx, &owner).await?.ok_or_else(|| ApiError::not_found("no training account"))?;
    // Read here, in the fill's own transaction: a stop or a partial close a
    // moment before leaves a different size to turn than the page last saw.
    let p = position(&tx, &owner, &symbol).await?.ok_or_else(|| ApiError::not_found(format!("no {symbol} position")))?;
    if p.qty * last < MIN_NOTIONAL {
        return Err(ApiError::bad_request(format!("an order is worth at least {MIN_NOTIONAL} USDT")));
    }
    let (need, freed) = turn_cost(&p, last);
    let free = available(&tx, &owner, &account).await? + freed;
    if need > free + 1e-6 {
        return Err(ApiError::bad_request(format!("not enough margin to reverse: this takes {need:.2} USDT, {free:.2} is free")));
    }
    let outcome = execute(&tx, &owner, &account, &symbol, p.side == Side::Short, 2.0 * p.qty, last, TAKER, None, None, "reversed").await?;
    tx.commit().await?;
    state.training.changed.notify_one();
    Ok(Json(json!({ "price": last, "qty": p.qty, "side": outcome.position.map(|p| p.side.id()) })))
}

/// `PATCH /api/training/positions/{symbol}` `{tp?, sl?}`: a position's take
/// profit and stop loss set, or cleared with null.
pub async fn exits(State(state): State<AppState>, headers: HeaderMap, Path(symbol): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let symbol = symbol_of(&symbol)?;
    let b = body.as_object().ok_or_else(|| ApiError::bad_request("body must be a JSON object"))?;
    let mut client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    let tx = client.transaction().await?;
    lock(&tx, &owner).await?.ok_or_else(|| ApiError::not_found("no training account"))?;
    let mut p = position(&tx, &owner, &symbol).await?.ok_or_else(|| ApiError::not_found(format!("no {symbol} position")))?;
    if b.contains_key("tp") {
        p.tp = price_field(b, "tp")?;
    }
    if b.contains_key("sl") {
        p.sl = price_field(b, "sl")?;
    }
    let reference = state.training.quote(&symbol).map_or(p.entry, |q| q.last);
    check_exits(p.side, reference, p.tp, p.sl)?;
    if p.sl.is_some_and(|sl| (sl - p.liquidation(mmr(&symbol))) * p.side.dir() <= 0.0) {
        return Err(ApiError::bad_request("the stop loss is past the liquidation price"));
    }
    save(&tx, &owner, &symbol, Some(&p)).await?;
    tx.commit().await?;
    state.training.changed.notify_one();
    Ok(Json(json!({ "tp": p.tp, "sl": p.sl })))
}

/// `PUT /api/training/leverage/{symbol}` `{leverage}`: a coin's leverage, 1 to
/// 100, set while it has no position and no order.
pub async fn leverage(State(state): State<AppState>, headers: HeaderMap, Path(symbol): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let symbol = symbol_of(&symbol)?;
    let lev = body.get("leverage").and_then(Value::as_i64).filter(|l| (1..=MAX_LEVERAGE).contains(l)).ok_or_else(|| ApiError::bad_request(format!("leverage is 1 to {MAX_LEVERAGE}")))?;
    let client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    ensure(&client, &owner).await?;
    let busy: bool = client
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM training_position WHERE owner = $1 AND symbol = $2) OR EXISTS (SELECT 1 FROM training_order WHERE owner = $1 AND symbol = $2)",
            &[&owner, &symbol],
        )
        .await?
        .get(0);
    if busy {
        return Err(ApiError::bad_request(format!("close the {symbol} position and cancel its orders to change its leverage")));
    }
    client
        .execute("UPDATE training_account SET leverage = jsonb_set(leverage, ARRAY[$2], to_jsonb($3::int)) WHERE owner = $1", &[&owner, &symbol, &(lev as i32)])
        .await?;
    Ok(Json(json!({ "symbol": symbol, "leverage": lev })))
}

/// `POST /api/training/reset`: a new run at 100,000, the open positions and
/// orders gone. The runs before stay in the history.
pub async fn reset(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let mut client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    ensure(&client, &owner).await?;
    let tx = client.transaction().await?;
    lock(&tx, &owner).await?;
    tx.execute("DELETE FROM training_position WHERE owner = $1", &[&owner]).await?;
    tx.execute("DELETE FROM training_order WHERE owner = $1", &[&owner]).await?;
    let run: i32 = tx
        .query_one("UPDATE training_account SET run = run + 1, wallet = $2, started_at = $3 WHERE owner = $1 RETURNING run", &[&owner, &START, &now_ms()])
        .await?
        .get(0);
    tx.commit().await?;
    state.training.changed.notify_one();
    Ok(Json(json!({ "run": run })))
}

#[derive(Deserialize)]
pub struct RunQuery {
    run: Option<i32>,
}

/// `GET /api/training/trades?run=`: a run's closed trades, latest first; the
/// current run's by default.
pub async fn history(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<RunQuery>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    account_only(&client, &owner).await?;
    ensure(&client, &owner).await?;
    let current: i32 = client.query_one("SELECT run FROM training_account WHERE owner = $1", &[&owner]).await?.get(0);
    let run = q.run.unwrap_or(current);
    let trades: Vec<Value> = client
        .query(
            "SELECT id, symbol, side, qty, entry, exit, leverage, margin, pnl, fees, funding, reason, tp, sl, opened_at, closed_at
             FROM training_trade WHERE owner = $1 AND run = $2 ORDER BY closed_at DESC, id DESC LIMIT $3",
            &[&owner, &run, &MAX_TRADES],
        )
        .await?
        .iter()
        .map(|r| {
            let (pnl, fees, funding): (f64, f64, f64) = (r.get(8), r.get(9), r.get(10));
            json!({
                "id": r.get::<_, i64>(0), "symbol": r.get::<_, String>(1), "side": r.get::<_, String>(2), "qty": r.get::<_, f64>(3),
                "entry": r.get::<_, f64>(4), "exit": r.get::<_, f64>(5), "leverage": r.get::<_, i32>(6), "margin": r.get::<_, f64>(7),
                "pnl": pnl, "fees": fees, "funding": funding, "net": pnl - fees + funding, "reason": r.get::<_, String>(11),
                "tp": r.get::<_, Option<f64>>(12), "sl": r.get::<_, Option<f64>>(13), "openedAt": r.get::<_, i64>(14), "closedAt": r.get::<_, i64>(15),
            })
        })
        .collect();
    let runs: Vec<Value> = client
        .query(
            "SELECT run, count(*), coalesce(sum(pnl - fees + funding), 0), min(opened_at), max(closed_at) FROM training_trade WHERE owner = $1 GROUP BY run ORDER BY run DESC",
            &[&owner],
        )
        .await?
        .iter()
        .map(|r| json!({ "run": r.get::<_, i32>(0), "trades": r.get::<_, i64>(1), "net": r.get::<_, f64>(2), "from": r.get::<_, Option<i64>>(3), "to": r.get::<_, Option<i64>>(4) }))
        .collect();
    Ok(Json(json!({ "run": run, "current": current, "trades": trades, "runs": runs })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(qty: f64, entry: f64, leverage: i64) -> Position {
        fill(None, true, qty, entry, 0.0, leverage, None, None, 0).position.unwrap()
    }

    #[test]
    fn an_order_the_positions_way_adds_at_the_average() {
        let first = fill(None, true, 1.0, 100.0, TAKER, 10, Some(120.0), Some(90.0), 1);
        let p = first.position.clone().unwrap();
        assert_eq!((p.qty, p.entry, p.margin, p.tp, p.sl), (1.0, 100.0, 10.0, Some(120.0), Some(90.0)));
        assert!((first.cash + 0.05).abs() < 1e-12, "the taker fee on 100");
        let more = fill(Some(p), true, 1.0, 110.0, 0.0, 99, None, None, 2).position.unwrap();
        assert_eq!((more.qty, more.entry, more.margin, more.leverage, more.peak), (2.0, 105.0, 21.0, 10, 2.0), "the position keeps its own leverage");
        assert_eq!((more.tp, more.sl, more.opened_at), (Some(120.0), Some(90.0), 1));
    }

    #[test]
    fn the_other_way_takes_off_closes_and_turns_around() {
        let p = long(2.0, 100.0, 10);
        let half = fill(Some(p), false, 1.0, 110.0, 0.0, 10, None, None, 5);
        let p = half.position.clone().unwrap();
        assert_eq!((half.cash, p.qty, p.margin, p.realized, half.closed), (10.0, 1.0, 10.0, 10.0, None));
        // Two more sold: the last coin closed at 90, then one short opened there.
        let turn = fill(Some(p), false, 2.0, 90.0, 0.0, 5, None, None, 9);
        let c = turn.closed.unwrap();
        assert_eq!((c.side, c.qty, c.entry, c.exit, c.pnl, c.margin), (Side::Long, 2.0, 100.0, 100.0, 0.0, 20.0), "+10 at 110, −10 at 90: out at an average 100");
        let s = turn.position.unwrap();
        assert_eq!((s.side, s.qty, s.entry, s.leverage, s.opened_at), (Side::Short, 1.0, 90.0, 5, 9));
        assert_eq!(turn.cash, -10.0);
    }

    #[test]
    fn reversing_closes_the_position_and_opens_as_much_the_other_way_bare() {
        let p = fill(None, true, 2.0, 100.0, 0.0, 10, Some(120.0), Some(90.0), 1).position.unwrap();
        // Closing at 110 gives back its 20 of margin and the 20 it made; the short puts up 22, and 0.22 of fees.
        let (need, freed) = turn_cost(&p, 110.0);
        assert!((need - 22.22).abs() < 1e-9 && (freed - 40.0).abs() < 1e-9);
        let turn = fill(Some(p), false, 4.0, 110.0, TAKER, 10, None, None, 7);
        let c = turn.closed.unwrap();
        assert_eq!((c.side, c.qty, c.exit, c.pnl), (Side::Long, 2.0, 110.0, 20.0));
        let s = turn.position.unwrap();
        assert_eq!((s.side, s.qty, s.entry, s.leverage, s.tp, s.sl, s.opened_at), (Side::Short, 2.0, 110.0, 10, None, None, 7), "its own take profit and stop loss do not carry over");
        assert!((turn.cash - (20.0 - 0.22)).abs() < 1e-9, "what it made, less a taker fee each way");
    }

    #[test]
    fn a_position_is_liquidated_where_its_margin_is_down_to_the_maintenance() {
        // 100x on 100: one coin's margin is 1, so a long goes near 99.4 at a 0.4% maintenance.
        let p = long(1.0, 100.0, 100);
        let at = p.liquidation(0.004);
        assert!((at - 99.0 / 0.996).abs() < 1e-9);
        assert!((1.0 + (at - 100.0) - at * 0.004).abs() < 1e-9, "what is left of the margin is the maintenance margin");
        let s = fill(None, false, 1.0, 100.0, 0.0, 100, None, None, 0).position.unwrap();
        let at = s.liquidation(0.004);
        assert!((1.0 + (100.0 - at) - at * 0.004).abs() < 1e-9);
        assert!(at > 100.0);
    }

    #[test]
    fn a_long_pays_a_positive_funding_rate_and_a_short_takes_it() {
        assert!((funding_of(Side::Long, 2.0, 100.0, 0.0001) + 0.02).abs() < 1e-12);
        assert!((funding_of(Side::Short, 2.0, 100.0, 0.0001) - 0.02).abs() < 1e-12);
    }

    #[test]
    fn the_price_reaches_orders_stops_and_targets() {
        let w = Watch {
            orders: vec![WatchedOrder { id: 1, symbol: "BTCUSDT".into(), buy: true, price: 95.0 }, WatchedOrder { id: 2, symbol: "BTCUSDT".into(), buy: false, price: 105.0 }],
            positions: vec![WatchedPosition { owner: "a".into(), symbol: "BTCUSDT".into(), side: Side::Long, tp: Some(110.0), sl: Some(90.0), liquidation: 80.0 }],
        };
        assert_eq!(w.due("BTCUSDT", 100.0), vec![]);
        assert_eq!(w.due("BTCUSDT", 95.0), vec![Trigger::Order(1)]);
        assert_eq!(w.due("BTCUSDT", 110.0), vec![Trigger::Order(2), Trigger::Exit { owner: "a".into(), symbol: "BTCUSDT".into(), price: 110.0, reason: "tp" }]);
        assert_eq!(w.due("BTCUSDT", 89.0)[1], Trigger::Exit { owner: "a".into(), symbol: "BTCUSDT".into(), price: 90.0, reason: "sl" });
        assert_eq!(w.due("ETHUSDT", 89.0), vec![]);
        assert_eq!(w.liquidations("BTCUSDT", 81.0), vec![]);
        assert_eq!(w.liquidations("BTCUSDT", 80.0).len(), 1);
    }

    #[test]
    fn exits_sit_on_the_right_sides() {
        assert!(check_exits(Side::Long, 100.0, Some(110.0), Some(90.0)).is_ok());
        assert!(check_exits(Side::Long, 100.0, Some(90.0), None).is_err());
        assert!(check_exits(Side::Short, 100.0, Some(90.0), Some(110.0)).is_ok());
        assert!(check_exits(Side::Short, 100.0, None, Some(95.0)).is_err());
    }

    #[test]
    fn a_mark_price_update_is_read() {
        let text = r#"{"e":"markPriceUpdate","E":1,"s":"BTCUSDT","p":"82000.10","i":"82001","P":"82002","r":"0.00010000","T":1792569600000}"#;
        assert_eq!(mark_update(text), Some(("BTCUSDT".into(), 82000.10, 0.0001, 1_792_569_600_000)));
        assert_eq!(mark_update(r#"{"e":"aggTrade","s":"BTCUSDT"}"#), None);
    }
}
