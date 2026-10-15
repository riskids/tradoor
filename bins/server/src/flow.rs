//! Net taker flow: what the buyers took minus what the sellers took, minute by
//! minute, and a word when it runs hard one way.
//!
//! Every trade names the side that crossed the spread, so the sum of buys less
//! sells says which side is paying up. Spot and perpetual are counted apart —
//! they are different markets with different money behind them, and the sizes
//! that matter differ by an order of magnitude, which is why each carries its
//! own threshold.
//!
//! Minutes are kept for two days, so the panel opens with history rather than
//! filling up as you watch. The crossings are per reader: the threshold is
//! theirs to set, and only the markets they asked for reach their Telegram.
//!
//! BTC is always followed. Another coin is followed once one of the server's
//! admins asks for it from the panel, on each exchange BTC is read from that
//! lists it, for a day or a week from when they last asked, or until they stop
//! it; then it lapses and its streams are closed, so what the server reads
//! stays what someone wants.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::{sleep, sleep_until, Instant};
use tokio_tungstenite::tungstenite::Message;
use tradoor_core::format::{format_price, format_turnover};
use tradoor_store::now_ms;
use tradoor_venues::{MarketRef, Venue};

use crate::api::{owner, ApiError, ApiResult, AppState};
use crate::catalog::Catalog;
use crate::dns::Doh;
use crate::telegram::{self, Telegram};
use crate::trades::{self, Print};

/// The markets followed, and what a minute's flow is measured against on each
/// until the reader says otherwise. The sizes differ by two orders of magnitude
/// between them — a quiet minute on Binance's perpetual is a busy hour on
/// Bitget's spot — so each threshold is about three times what a minute's flow
/// usually comes to there, measured on the live streams.
const MARKETS: [(&str, f64); 11] = [
    ("binance-spot:BTCUSDT", 1_000_000.0),
    ("binance-perp:BTCUSDT", 5_000_000.0),
    ("bybit-spot:BTCUSDT", 1_000_000.0),
    ("bybit-perp:BTCUSDT", 2_000_000.0),
    ("okx-spot:BTCUSDT", 200_000.0),
    ("okx-perp:BTCUSDT", 2_500_000.0),
    ("bitget-spot:BTCUSDT", 150_000.0),
    ("bitget-perp:BTCUSDT", 1_500_000.0),
    ("hyperliquid-perp:BTCUSD", 2_000_000.0),
    ("kraken-spot:BTCUSD", 400_000.0),
    ("kraken-perp:BTCUSD", 100_000.0),
];
/// The coin always followed; `MARKETS` are its markets.
const ALWAYS: &str = "BTC";
/// How long a coin asked for is followed before it lapses: a day, unless a
/// week or no end is asked for (`Span`).
const FOLLOW_MS: i64 = 24 * 3_600_000;
const FOLLOW_WEEK_MS: i64 = 7 * FOLLOW_MS;
/// Coins followed at once besides BTC: each is up to eleven more streams.
const MAX_COINS: i64 = 10;
/// A new coin's threshold until a few minutes of its flow say what is usual.
const FRESH_THRESHOLD: f64 = 100_000.0;
/// How far back a crossing looks: the flow of the last minute.
const WINDOW_MS: i64 = 60_000;
/// Minutes kept, and the crossings with them.
const KEPT_MS: i64 = 48 * 3_600_000;
/// How often the flow is weighed against the thresholds.
const WEIGH: Duration = Duration::from_secs(5);
/// How often minutes are written down, and the readers' settings read back.
const FLUSH: Duration = Duration::from_secs(10);
const RETRY_SECS: [u64; 5] = [1, 3, 5, 10, 30];
const SILENCE: Duration = Duration::from_secs(90);
const NO_TRADES: Duration = Duration::from_secs(150);
const PING: Duration = Duration::from_secs(15);
/// Crossings kept per reader.
const EVENTS_KEPT: i64 = 500;
/// A threshold below this is noise, and above it is nobody's alert.
const MIN_THRESHOLD: f64 = 10_000.0;
const MAX_THRESHOLD: f64 = 1e10;

/// BTC's markets, in the order the panel shows them.
fn btc_markets() -> Vec<String> {
    MARKETS.iter().map(|(market, _)| (*market).to_string()).collect()
}

/// BTC's thresholds, measured once and written down.
fn fixed_threshold(market: &str) -> Option<f64> {
    MARKETS.iter().find(|(m, _)| *m == market).map(|(_, t)| *t)
}

/// A coin as a chart names it, its symbol less the quote: `ETH`, `1000PEPE`.
fn valid_coin(coin: &str) -> bool {
    (1..=20).contains(&coin.len()) && coin.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// Where a coin is looked for: each venue BTC is followed on, quoted as BTC is
/// there, and as a thousand of it where a venue lists only that (`1000PEPE`).
fn candidates(coin: &str) -> Vec<[String; 2]> {
    MARKETS
        .iter()
        .filter_map(|(market, _)| {
            let (venue, symbol) = market.split_once(':')?;
            let quote = symbol.strip_prefix(ALWAYS)?;
            Some([format!("{venue}:{coin}{quote}"), format!("{venue}:1000{coin}{quote}")])
        })
        .collect()
}

/// The markets a coin is followed on: the first of each venue's candidates the
/// catalogue lists.
async fn coin_markets(catalog: &Catalog, coin: &str) -> Vec<String> {
    let candidates = candidates(coin);
    let listed = catalog.listed(&candidates.iter().flatten().cloned().collect::<Vec<_>>()).await;
    candidates.into_iter().filter_map(|pair| pair.into_iter().find(|m| listed.contains(m))).collect()
}

/// A size rounded to one, two or five of its power of ten, as a person would set it.
fn round_size(usd: f64) -> f64 {
    let power = 10f64.powf(usd.log10().floor());
    let lead = usd / power;
    power * if lead < 1.5 { 1.0 } else if lead < 3.5 { 2.0 } else if lead < 7.5 { 5.0 } else { 10.0 }
}

/// What a trade was worth. Everywhere but OKX's swaps a size is coins, and the
/// money is price times size; there it is contracts of a hundredth of a coin,
/// so a print of fifty is half a coin and not fifty of them.
fn notional(venue: Venue, price: f64, qty: f64, contract: f64) -> f64 {
    if venue == Venue::OkxPerp { price * qty * contract } else { price * qty }
}

/// A coin followed besides BTC: until when — none until it is stopped — and on which markets.
#[derive(Clone, Debug)]
struct Followed {
    coin: String,
    ends_at: Option<i64>,
    markets: Vec<String>,
}

/// What one reader wants of one market.
#[derive(Clone, Debug, PartialEq)]
pub struct Watch {
    pub owner: String,
    pub market: String,
    pub threshold: f64,
    pub notify: bool,
}

/// The minute being filled, and the trades still inside the window.
#[derive(Default)]
struct Live {
    buy: f64,
    sell: f64,
    trades: i64,
    minute: i64,
    price: f64,
    /// Signed dollars and when, oldest first: the window slides over these.
    recent: VecDeque<(i64, f64)>,
}

impl Live {
    fn delta(&self, since: i64) -> f64 {
        self.recent.iter().filter(|(at, _)| *at >= since).map(|(_, usd)| usd).sum()
    }
}

pub struct Flow {
    pool: deadpool_postgres::Pool,
    doh: Arc<Doh>,
    /// The exchanges' own names for the markets followed.
    catalog: Arc<Catalog>,
    /// Reaches the exchanges past a blocked resolver, for what is not streamed.
    http: reqwest::Client,
    telegram: Option<Arc<Telegram>>,
    /// Per market, the minute in hand. Written down every few seconds.
    live: Mutex<HashMap<String, Live>>,
    /// Minutes not yet written, as `(market, minute) -> (buy, sell, trades, price)`.
    pending: Mutex<HashMap<(String, i64), (f64, f64, i64, f64)>>,
    /// What the readers asked for, read back from the table now and then.
    watches: Mutex<Vec<Watch>>,
    /// Markets already reported, per reader and side, until the flow calms.
    said: Mutex<HashMap<(String, String, bool), ()>>,
    /// The coins in one contract, where a size is counted in contracts.
    contracts: Mutex<HashMap<String, f64>>,
    /// Browser ids that may choose the coins followed.
    admins: Vec<String>,
    /// The coins followed besides BTC, as last read from `flow_coin`.
    coins: Mutex<Vec<Followed>>,
    /// The stream reading each market followed, to close when it is not.
    streams: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    /// What a minute's flow usually comes to on each market, over the last day.
    typical: Mutex<HashMap<String, f64>>,
}

impl Flow {
    pub fn start(pool: deadpool_postgres::Pool, doh: Arc<Doh>, catalog: Arc<Catalog>, telegram: Option<Arc<Telegram>>, admins: Vec<String>) -> Arc<Self> {
        let http = reqwest::Client::builder().dns_resolver(doh.resolver()).timeout(Duration::from_secs(20)).build().unwrap_or_default();
        let flow = Arc::new(Self {
            pool,
            doh,
            catalog,
            http,
            telegram,
            live: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            watches: Mutex::new(Vec::new()),
            said: Mutex::new(HashMap::new()),
            contracts: Mutex::new(HashMap::new()),
            admins,
            coins: Mutex::new(Vec::new()),
            streams: Mutex::new(HashMap::new()),
            typical: Mutex::new(HashMap::new()),
        });
        if let Ok(mut streams) = flow.streams.lock() {
            for market in btc_markets() {
                streams.insert(market.clone(), tokio::spawn(follow(market, flow.clone())).abort_handle());
            }
        }
        tokio::spawn(keep(flow.clone()));
        tokio::spawn(weigh(flow.clone()));
        tracing::info!(markets = ?btc_markets(), admins = flow.admins.len(), "watching flow");
        flow
    }

    pub fn is_admin(&self, owner: &str) -> bool {
        self.admins.iter().any(|a| a == owner)
    }

    /// Every market followed: BTC's, then each coin's in the order they were asked for.
    fn followed(&self) -> Vec<String> {
        let mut out = btc_markets();
        if let Ok(coins) = self.coins.lock() {
            out.extend(coins.iter().flat_map(|c| c.markets.iter().cloned()));
        }
        out
    }

    fn follows(&self, market: &str) -> bool {
        self.followed().iter().any(|m| m == market)
    }

    /// One coin's markets as the panel lists them; none for a coin not followed.
    fn markets_of(&self, coin: &str) -> Vec<String> {
        if coin == ALWAYS {
            return btc_markets();
        }
        self.coins.lock().ok().and_then(|c| c.iter().find(|f| f.coin == coin).map(|f| f.markets.clone())).unwrap_or_default()
    }

    /// Brings the streams in line with the coins asked for: a coin added has its
    /// markets read, one lapsed or stopped has them closed.
    async fn sync(self: &Arc<Self>) -> anyhow::Result<()> {
        let rows = self.pool.get().await?.query("SELECT coin, ends_at FROM flow_coin WHERE ends_at IS NULL OR ends_at > $1 ORDER BY added_at", &[&now_ms()]).await?;
        let held = self.coins.lock().map(|c| c.clone()).unwrap_or_default();
        let mut coins = Vec::new();
        for row in rows {
            let (coin, ends_at): (String, Option<i64>) = (row.get(0), row.get(1));
            // Where a coin trades is asked of the catalogue once, not every pass.
            let markets = match held.iter().find(|f| f.coin == coin && !f.markets.is_empty()) {
                Some(f) => f.markets.clone(),
                None => coin_markets(&self.catalog, &coin).await,
            };
            coins.push(Followed { coin, ends_at, markets });
        }
        if let Ok(mut held) = self.coins.lock() {
            *held = coins;
        }
        let wanted = self.followed();
        let Ok(mut streams) = self.streams.lock() else { return Ok(()) };
        streams.retain(|market, stream| {
            let keep = wanted.contains(market);
            if !keep {
                stream.abort();
                tracing::info!(market, "flow no longer followed");
            }
            keep
        });
        for market in wanted {
            if !streams.contains_key(&market) {
                streams.insert(market.clone(), tokio::spawn(follow(market, self.clone())).abort_handle());
            }
        }
        Ok(())
    }

    /// What a minute's flow usually comes to on each market over the last day.
    async fn refresh_typical(&self) -> anyhow::Result<()> {
        let rows = self
            .pool
            .get()
            .await?
            .query(
                "SELECT market, percentile_cont(0.5) WITHIN GROUP (ORDER BY abs(buy_usd - sell_usd)) FROM flow_minute WHERE minute >= $1 GROUP BY market",
                &[&(now_ms() - 24 * 3_600_000)],
            )
            .await?;
        if let Ok(mut typical) = self.typical.lock() {
            *typical = rows.iter().map(|r| (r.get::<_, String>(0), r.get::<_, f64>(1))).collect();
        }
        Ok(())
    }

    /// What a minute's flow is weighed against until the reader says otherwise:
    /// BTC's sizes as measured, another coin's three times its usual minute.
    fn default_threshold(&self, market: &str) -> f64 {
        fixed_threshold(market).unwrap_or_else(|| {
            let usual = self.typical.lock().ok().and_then(|t| t.get(market).copied()).unwrap_or(0.0);
            if usual > 0.0 { round_size(3.0 * usual).clamp(MIN_THRESHOLD, MAX_THRESHOLD) } else { FRESH_THRESHOLD }
        })
    }

    /// One trade, counted to the side that took it.
    fn record(&self, market: &str, print: &Print<'_>, usd: f64, at: i64) {
        let Ok(mut live) = self.live.lock() else { return };
        let entry = live.entry(market.to_string()).or_default();
        let minute = at - at.rem_euclid(60_000);
        if entry.minute != minute {
            entry.minute = minute;
            (entry.buy, entry.sell, entry.trades) = (0.0, 0.0, 0);
        }
        if print.buy {
            entry.buy += usd;
        } else {
            entry.sell += usd;
        }
        entry.trades += 1;
        entry.price = print.price;
        entry.recent.push_back((at, if print.buy { usd } else { -usd }));
        while entry.recent.front().is_some_and(|(t, _)| *t < at - WINDOW_MS) {
            entry.recent.pop_front();
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert((market.to_string(), minute), (entry.buy, entry.sell, entry.trades, entry.price));
        }
    }

    /// The coins in one OKX swap contract, asked for once and kept: a size
    /// there is contracts, and a hundredth of a coin is not a coin.
    async fn contract_size(&self, native: &str) -> f64 {
        if let Some(size) = self.contracts.lock().ok().and_then(|c| c.get(native).copied()) {
            return size;
        }
        let sizes = async {
            let body = self.http.get("https://www.okx.com/api/v5/public/instruments?instType=SWAP").send().await.ok()?.text().await.ok()?;
            Some(crate::liquidations::parse_okx_sizes(&serde_json::from_str::<Value>(&body).ok()?))
        }
        .await
        .unwrap_or_default();
        let size = sizes.get(native).copied().unwrap_or(1.0);
        if let Ok(mut held) = self.contracts.lock() {
            held.extend(sizes);
        }
        size
    }

    /// The flow of the last minute on each market followed, with the last price.
    pub fn now(&self) -> Vec<(String, f64, f64)> {
        let since = now_ms() - WINDOW_MS;
        let markets = self.followed();
        let live = self.live.lock().ok();
        markets
            .into_iter()
            .map(|market| {
                let entry = live.as_ref().and_then(|l| l.get(&market));
                (market, entry.map_or(0.0, |e| e.delta(since)), entry.map_or(0.0, |e| e.price))
            })
            .collect()
    }
}

/// Keeps one exchange connected for as long as the server runs.
async fn follow(market: String, flow: Arc<Flow>) {
    let Some(venue) = MarketRef::parse(&market).map(|m| m.venue) else { return };
    let urls = trades::stream_urls(venue);
    if urls.is_empty() {
        return;
    }
    let (mut attempt, mut at) = (0, 0);
    loop {
        let url = urls[at % urls.len()];
        if let Err(e) = session(&market, venue, url, &flow).await {
            tracing::warn!(venue = venue.id(), url, error = %e, "flow stream lost");
            sleep(Duration::from_secs(RETRY_SECS[attempt.min(RETRY_SECS.len() - 1)])).await;
            (attempt, at) = (attempt + 1, at + 1);
        } else {
            attempt = 0;
        }
    }
}

async fn session(market: &str, venue: Venue, url: &str, flow: &Flow) -> anyhow::Result<()> {
    let symbol = MarketRef::parse(market).map(|m| m.symbol).unwrap_or_default();
    // Asked for in the exchange's own name, and answered in it: OKX says
    // BTC-USDT-SWAP, Hyperliquid BTC, Kraken PF_XBTUSD.
    let theirs = if trades::renames(venue) {
        flow.catalog.native_names(venue, std::slice::from_ref(&symbol)).await.get(&symbol).cloned().unwrap_or_else(|| symbol.clone())
    } else {
        symbol.clone()
    };
    // What one contract holds, where a size is not coins.
    let contract = if venue == Venue::OkxPerp { flow.contract_size(&theirs).await } else { 1.0 };

    let connect = flow.doh.connect_ws(url);
    let mut ws = tokio::time::timeout(Duration::from_secs(15), connect).await.map_err(|_| anyhow::anyhow!("connecting timed out"))??;
    for frame in trades::subscribe(venue, std::slice::from_ref(&theirs)) {
        ws.send(Message::text(frame)).await?;
    }
    tracing::info!(venue = venue.id(), symbol = %theirs, contract, "reading flow");
    let (mut heard, mut traded) = (Instant::now(), Instant::now());
    let mut ping = tokio::time::interval(PING);
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
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                // The borrowed symbols live as long as the frame does.
                let mut prints = Vec::new();
                trades::read_prints(venue, &value, &mut prints);
                let now = now_ms();
                for print in &prints {
                    if print.symbol != theirs {
                        continue;
                    }
                    traded = Instant::now();
                    // An exchange's clock can be a little ahead; the minute a
                    // trade lands in is ours, so the panel and the table agree.
                    flow.record(market, print, notional(venue, print.price, print.qty, contract), now);
                }
            }
            _ = ping.tick() => {
                if let Some(frame) = trades::ping(venue) {
                    ws.send(Message::text(frame)).await?;
                }
            }
            () = sleep_until(heard + SILENCE) => anyhow::bail!("nothing heard for {} seconds", SILENCE.as_secs()),
            () = sleep_until(traded + NO_TRADES) => anyhow::bail!("no trade in {} seconds", NO_TRADES.as_secs()),
        }
    }
}

/// Writes the minutes down, follows the coins asked for, trims what is older
/// than two days, and reads back what the readers asked for.
async fn keep(flow: Arc<Flow>) {
    let mut trimmed: Option<Instant> = None;
    loop {
        sleep(FLUSH).await;
        if let Err(e) = write_minutes(&flow).await {
            tracing::warn!(error = %e, "writing flow minutes");
        }
        if let Err(e) = flow.sync().await {
            tracing::warn!(error = %e, "following the coins asked for");
        }
        match read_watches(&flow.pool).await {
            Ok(watches) => {
                if let Ok(mut held) = flow.watches.lock() {
                    *held = watches;
                }
            }
            Err(e) => tracing::warn!(error = %e, "reading flow watches"),
        }
        if trimmed.is_none_or(|t| t.elapsed() >= Duration::from_secs(600)) {
            trimmed = Some(Instant::now());
            if let Err(e) = trim(&flow.pool).await {
                tracing::warn!(error = %e, "trimming flow");
            }
            if let Err(e) = flow.refresh_typical().await {
                tracing::warn!(error = %e, "measuring the usual flow");
            }
        }
    }
}

async fn write_minutes(flow: &Flow) -> anyhow::Result<()> {
    let rows: Vec<((String, i64), (f64, f64, i64, f64))> = {
        let Ok(mut pending) = flow.pending.lock() else { return Ok(()) };
        pending.drain().collect()
    };
    if rows.is_empty() {
        return Ok(());
    }
    let client = flow.pool.get().await?;
    let (markets, minutes): (Vec<String>, Vec<i64>) = rows.iter().map(|((m, t), _)| (m.clone(), *t)).unzip();
    let (buys, sells): (Vec<f64>, Vec<f64>) = rows.iter().map(|(_, (b, s, _, _))| (*b, *s)).unzip();
    let counts: Vec<i64> = rows.iter().map(|(_, (_, _, n, _))| *n).collect();
    let prices: Vec<f64> = rows.iter().map(|(_, (_, _, _, p))| *p).collect();
    client
        .execute(
            "INSERT INTO flow_minute (market, minute, buy_usd, sell_usd, trades, price)
             SELECT * FROM UNNEST($1::text[], $2::bigint[], $3::double precision[], $4::double precision[], $5::bigint[], $6::double precision[])
             ON CONFLICT (market, minute) DO UPDATE SET buy_usd = EXCLUDED.buy_usd, sell_usd = EXCLUDED.sell_usd, trades = EXCLUDED.trades, price = EXCLUDED.price",
            &[&markets, &minutes, &buys, &sells, &counts, &prices],
        )
        .await?;
    Ok(())
}

async fn trim(pool: &deadpool_postgres::Pool) -> anyhow::Result<()> {
    let client = pool.get().await?;
    let cutoff = now_ms() - KEPT_MS;
    client.execute("DELETE FROM flow_minute WHERE minute < $1", &[&cutoff]).await?;
    client.execute("DELETE FROM flow_event WHERE at < $1", &[&cutoff]).await?;
    client.execute("DELETE FROM flow_coin WHERE ends_at < $1", &[&cutoff]).await?;
    Ok(())
}

async fn read_watches(pool: &deadpool_postgres::Pool) -> anyhow::Result<Vec<Watch>> {
    let client = pool.get().await?;
    let rows = client.query("SELECT owner, market, threshold_usd, notify FROM flow_watch", &[]).await?;
    Ok(rows.iter().map(|r| Watch { owner: r.get(0), market: r.get(1), threshold: r.get(2), notify: r.get(3) }).collect())
}

/// Weighs each market's last minute against what each reader asked for. A
/// market that has spoken says nothing more until its flow falls back to half
/// the threshold, so one push does not ring over and over.
async fn weigh(flow: Arc<Flow>) {
    loop {
        sleep(WEIGH).await;
        let now = flow.now();
        let watches = flow.watches.lock().map(|w| w.clone()).unwrap_or_default();
        for watch in watches {
            let Some((_, delta, price)) = now.iter().find(|(m, _, _)| *m == watch.market) else { continue };
            let (delta, price) = (*delta, *price);
            let buying = delta > 0.0;
            let key = (watch.owner.clone(), watch.market.clone(), buying);
            let loud = delta.abs() >= watch.threshold;
            let calm = delta.abs() < watch.threshold / 2.0;
            let said = flow.said.lock().map(|s| s.contains_key(&key)).unwrap_or(false);
            if calm {
                if let Ok(mut s) = flow.said.lock() {
                    s.remove(&key);
                    s.remove(&(watch.owner.clone(), watch.market.clone(), !buying));
                }
            }
            if !loud || said || price <= 0.0 {
                continue;
            }
            if let Ok(mut s) = flow.said.lock() {
                s.insert(key, ());
            }
            if let Err(e) = report(&flow, &watch, delta, price).await {
                tracing::warn!(market = %watch.market, error = %e, "recording a flow crossing");
            }
        }
    }
}

async fn report(flow: &Flow, watch: &Watch, delta: f64, price: f64) -> anyhow::Result<()> {
    let at = now_ms();
    let client = flow.pool.get().await?;
    client
        .execute(
            "INSERT INTO flow_event (owner, market, at, delta_usd, window_ms, price) VALUES ($1, $2, $3, $4, $5, $6)",
            &[&watch.owner, &watch.market, &at, &delta, &(WINDOW_MS as i32), &price],
        )
        .await?;
    client
        .execute(
            "DELETE FROM flow_event WHERE owner = $1 AND id NOT IN (SELECT id FROM flow_event WHERE owner = $1 ORDER BY at DESC, id DESC LIMIT $2)",
            &[&watch.owner, &EVENTS_KEPT],
        )
        .await?;
    tracing::info!(market = %watch.market, delta, notify = watch.notify, "flow crossing");
    if let Some(telegram) = flow.telegram.as_ref().filter(|_| watch.notify) {
        let text = |offset: i32| flow_text(&watch.market, delta, price, at, offset);
        telegram.send_to_owner(&watch.owner, text).await;
    }
    Ok(())
}

/// What a crossing says, in the page's own wording and the reader's hours.
pub fn flow_text(market: &str, delta: f64, price: f64, at: i64, offset: i32) -> String {
    let (way, side) = if delta > 0.0 { ("🟢", "bought") } else { ("🔴", "sold") };
    let name = MarketRef::parse(market).map_or_else(|| market.to_string(), |m| format!("{} {}", m.symbol, if m.venue.id().ends_with("perp") { "perp" } else { "spot" }));
    format!(
        "{way} {name} — {} net {side} in a minute\nprice {} — {}",
        format_turnover(delta.abs()),
        format_price(price, None),
        telegram::stamp(at, offset),
    )
}

// ---------------------------------------------------------------- the API

fn row_json(r: &tokio_postgres::Row) -> Value {
    json!({ "id": r.get::<_, i64>(0), "market": r.get::<_, String>(1), "at": r.get::<_, i64>(2), "deltaUsd": r.get::<_, f64>(3), "windowMs": r.get::<_, i32>(4), "price": r.get::<_, f64>(5) })
}

#[derive(serde::Deserialize)]
pub struct StateQuery {
    coin: Option<String>,
}

/// `GET /api/flow?coin=ETH`: one coin's flow on every market it is followed on,
/// now and over the last day, this reader's settings and the crossings they
/// were told about there, and the coins followed. A coin no longer followed
/// answers with BTC's, which always is.
pub async fn state(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<StateQuery>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let Some(flow) = state.flow.as_ref() else {
        return Ok(Json(json!({ "watching": false, "markets": [], "watches": [], "events": [] })));
    };
    let asked = q.coin.unwrap_or_default().trim().to_ascii_uppercase();
    let coin = if flow.markets_of(&asked).is_empty() { ALWAYS.to_string() } else { asked };
    let wanted = flow.markets_of(&coin);
    let client = state.pool.get().await?;
    let now = now_ms();
    let sums = client
        .query(
            "SELECT market,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $1) AS m5,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $2) AS m15,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $3) AS h1,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $4) AS h4,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $5) AS h12,
                    sum(buy_usd - sell_usd) AS day
             FROM flow_minute WHERE minute >= $6 AND market = ANY($7) GROUP BY market",
            &[&(now - 5 * 60_000), &(now - 15 * 60_000), &(now - 3_600_000), &(now - 4 * 3_600_000), &(now - 12 * 3_600_000), &(now - 24 * 3_600_000), &wanted],
        )
        .await?;
    let sum = |market: &str, at: usize| sums.iter().find(|r| r.get::<_, String>(0) == market).and_then(|r| r.get::<_, Option<f64>>(at)).unwrap_or(0.0);
    let watches = client.query("SELECT market, threshold_usd, notify FROM flow_watch WHERE owner = $1 AND market = ANY($2)", &[&owner, &wanted]).await?;
    let events = client
        .query(
            "SELECT id, market, at, delta_usd, window_ms, price FROM flow_event WHERE owner = $1 AND market = ANY($3) ORDER BY at DESC, id DESC LIMIT $2",
            &[&owner, &EVENTS_KEPT, &wanted],
        )
        .await?;
    let live = flow.now();
    let markets = wanted
        .iter()
        .map(|market| {
            let (minute, price) = live.iter().find(|(m, _, _)| m == market).map_or((0.0, 0.0), |(_, minute, price)| (*minute, *price));
            let watch = watches.iter().find(|r| r.get::<_, String>(0) == *market);
            json!({
                "market": market,
                "price": price,
                "minute": minute,
                "m5": sum(market, 1),
                "m15": sum(market, 2),
                "h1": sum(market, 3),
                "h4": sum(market, 4),
                "h12": sum(market, 5),
                "day": sum(market, 6),
                "thresholdUsd": watch.map_or_else(|| flow.default_threshold(market), |r| r.get::<_, f64>(1)),
                "notify": watch.is_some_and(|r| r.get::<_, bool>(2)),
            })
        })
        .collect::<Vec<_>>();
    let mut coins = vec![json!({ "coin": ALWAYS, "until": null })];
    if let Ok(held) = flow.coins.lock() {
        coins.extend(held.iter().filter(|c| !c.markets.is_empty()).map(|c| json!({ "coin": c.coin, "until": c.ends_at })));
    }
    Ok(Json(json!({
        "watching": true,
        "windowMs": WINDOW_MS,
        "coin": coin,
        "coins": coins,
        "admin": flow.is_admin(&owner),
        "markets": markets,
        "events": events.iter().map(row_json).collect::<Vec<_>>(),
    })))
}

#[derive(serde::Deserialize)]
pub struct FollowQuery {
    #[serde(rename = "for")]
    span: Option<String>,
}

/// `PUT /api/flow/coins/{coin}?for=day|week|always`: follow a coin's flow for
/// the next day (the default), the next week, or until it is stopped — from
/// now, if it already is. The server's admins choose.
pub async fn follow_coin(State(state): State<AppState>, headers: HeaderMap, Path(coin): Path<String>, Query(q): Query<FollowQuery>) -> ApiResult<Json<Value>> {
    let flow = admin(&state, &headers)?;
    let coin = coin.trim().to_ascii_uppercase();
    if !valid_coin(&coin) {
        return Err(ApiError::bad_request("a coin is its ticker less the quote, like ETH or 1000PEPE"));
    }
    if coin == ALWAYS {
        return Err(ApiError::bad_request("BTC is always followed"));
    }
    let now = now_ms();
    let ends_at = match q.span.as_deref().unwrap_or("day") {
        "day" => Some(now + FOLLOW_MS),
        "week" => Some(now + FOLLOW_WEEK_MS),
        "always" => None,
        _ => return Err(ApiError::bad_request("for is day, week or always")),
    };
    let markets = coin_markets(&flow.catalog, &coin).await;
    if markets.is_empty() {
        return Err(ApiError::bad_request(format!("none of the exchanges flow is read from lists {coin}")));
    }
    let client = state.pool.get().await?;
    let others: i64 = client.query_one("SELECT count(*) FROM flow_coin WHERE (ends_at IS NULL OR ends_at > $1) AND coin <> $2", &[&now, &coin]).await?.get(0);
    if others >= MAX_COINS {
        return Err(ApiError::bad_request(format!("{MAX_COINS} coins are followed already — stop one first")));
    }
    // A coin still followed keeps its place in the list; one that lapsed comes back at the end.
    client
        .execute(
            "INSERT INTO flow_coin (coin, added_by, added_at, ends_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (coin) DO UPDATE SET ends_at = EXCLUDED.ends_at, added_by = EXCLUDED.added_by,
                 added_at = CASE WHEN flow_coin.ends_at IS NULL OR flow_coin.ends_at > EXCLUDED.added_at THEN flow_coin.added_at ELSE EXCLUDED.added_at END",
            &[&coin, &owner(&headers)?, &now, &ends_at],
        )
        .await?;
    if let Err(e) = flow.sync().await {
        tracing::warn!(error = %e, "following the coins asked for");
    }
    tracing::info!(coin, markets = markets.len(), until = ?ends_at, "following a coin's flow");
    Ok(Json(json!({ "coin": coin, "until": ends_at, "markets": markets })))
}

/// `DELETE /api/flow/coins/{coin}`: stop following a coin before its time is up.
pub async fn unfollow_coin(State(state): State<AppState>, headers: HeaderMap, Path(coin): Path<String>) -> ApiResult<Json<Value>> {
    let flow = admin(&state, &headers)?;
    let coin = coin.trim().to_ascii_uppercase();
    if coin == ALWAYS {
        return Err(ApiError::bad_request("BTC is always followed"));
    }
    state.pool.get().await?.execute("DELETE FROM flow_coin WHERE coin = $1", &[&coin]).await?;
    if let Err(e) = flow.sync().await {
        tracing::warn!(error = %e, "following the coins asked for");
    }
    tracing::info!(coin, "no longer following a coin's flow");
    Ok(Json(json!({ "coin": coin })))
}

/// The flow collector, for one of the server's admins only.
fn admin<'a>(state: &'a AppState, headers: &HeaderMap) -> ApiResult<&'a Arc<Flow>> {
    let owner = owner(headers)?;
    let flow = state.flow.as_ref().ok_or_else(|| ApiError::not_found("this server follows no flow"))?;
    if !flow.is_admin(&owner) {
        return Err(ApiError::forbidden("only this server's admins choose the coins whose flow it follows"));
    }
    Ok(flow)
}

/// `GET /api/flow/minutes?market=…`: the minutes of the last two days whose
/// flow ran harder than this reader's threshold, for the chart to mark.
pub async fn minutes(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<MinutesQuery>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let Some(flow) = state.flow.as_ref().filter(|f| f.follows(&q.market)) else {
        return Ok(Json(json!({ "market": q.market, "thresholdUsd": 0.0, "minutes": [] })));
    };
    let client = state.pool.get().await?;
    let watch = client.query_opt("SELECT threshold_usd FROM flow_watch WHERE owner = $1 AND market = $2", &[&owner, &q.market]).await?;
    let threshold = watch.map_or_else(|| flow.default_threshold(&q.market), |r| r.get::<_, f64>(0));
    let rows = client
        .query(
            "SELECT minute, buy_usd - sell_usd AS delta, price FROM flow_minute
             WHERE market = $1 AND minute >= $2 AND abs(buy_usd - sell_usd) >= $3 AND price > 0
             ORDER BY minute",
            &[&q.market, &(now_ms() - KEPT_MS), &threshold],
        )
        .await?;
    let minutes: Vec<Value> = rows.iter().map(|r| json!({ "at": r.get::<_, i64>(0), "deltaUsd": r.get::<_, f64>(1), "price": r.get::<_, f64>(2) })).collect();
    Ok(Json(json!({ "market": q.market, "thresholdUsd": threshold, "minutes": minutes })))
}

#[derive(serde::Deserialize)]
pub struct MinutesQuery {
    market: String,
}

/// `PUT /api/flow/{market}`: what this reader wants of one market.
pub async fn watch(State(state): State<AppState>, headers: HeaderMap, Path(market): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let Some(flow) = state.flow.as_ref().filter(|f| f.follows(&market)) else {
        return Err(ApiError::bad_request("this server does not follow that market's flow"));
    };
    let threshold = body["thresholdUsd"].as_f64().unwrap_or_else(|| flow.default_threshold(&market));
    if !(MIN_THRESHOLD..=MAX_THRESHOLD).contains(&threshold) {
        return Err(ApiError::bad_request(format!("thresholdUsd must be between {MIN_THRESHOLD} and {MAX_THRESHOLD}")));
    }
    let notify = body["notify"].as_bool().unwrap_or(true);
    let client = state.pool.get().await?;
    client
        .execute(
            "INSERT INTO flow_watch (owner, market, threshold_usd, notify) VALUES ($1, $2, $3, $4)
             ON CONFLICT (owner, market) DO UPDATE SET threshold_usd = EXCLUDED.threshold_usd, notify = EXCLUDED.notify",
            &[&owner, &market, &threshold, &notify],
        )
        .await?;
    Ok(Json(json!({ "market": market, "thresholdUsd": threshold, "notify": notify })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_market_is_measured_against_its_own_size() {
        let markets = btc_markets();
        assert_eq!(markets.len(), 11);
        assert!(markets.contains(&"binance-perp:BTCUSDT".to_string()) && markets.contains(&"hyperliquid-perp:BTCUSD".to_string()));
        assert!(markets.iter().all(|m| MarketRef::parse(m).is_some_and(|r| !trades::stream_urls(r.venue).is_empty())), "every one of them has a stream to read");
        assert!(fixed_threshold("binance-perp:BTCUSDT") > fixed_threshold("binance-spot:BTCUSDT"), "a perpetual turns over more, so it takes more to speak");
        assert_eq!(fixed_threshold("binance-perp:ETHUSDT"), None, "another coin's is measured, not written down");
    }

    #[test]
    fn another_coin_is_looked_for_where_btc_is_followed() {
        let found = candidates("ETH");
        assert_eq!(found.len(), MARKETS.len());
        assert_eq!(found[0], ["binance-spot:ETHUSDT".to_string(), "binance-spot:1000ETHUSDT".to_string()]);
        assert!(found.iter().any(|pair| pair[0] == "hyperliquid-perp:ETHUSD"), "quoted as BTC is there");
        assert!(found.iter().flatten().all(|m| MarketRef::parse(m).is_some()), "and every one a market the streams can read");
        assert!(valid_coin("XPL") && valid_coin("1000PEPE"));
        assert!(!valid_coin("") && !valid_coin("eth") && !valid_coin("BTC/USD") && !valid_coin("ABCDEFGHIJKLMNOPQRSTU"));
    }

    #[test]
    fn a_threshold_is_rounded_as_a_person_would_set_it() {
        assert_eq!(round_size(2_700_000.0), 2_000_000.0);
        assert_eq!(round_size(380_000.0), 500_000.0);
        assert_eq!(round_size(120_000.0), 100_000.0);
        assert_eq!(round_size(8_000.0), 10_000.0);
    }

    #[test]
    fn a_contract_is_not_a_coin() {
        assert_eq!(notional(Venue::BinancePerp, 80_000.0, 0.5, 1.0), 40_000.0);
        assert_eq!(notional(Venue::OkxPerp, 80_000.0, 50.0, 0.01), 40_000.0, "fifty contracts of a hundredth of a coin");
        for venue in [Venue::KrakenPerp, Venue::HyperliquidPerp, Venue::BitgetPerp, Venue::KrakenSpot] {
            assert_eq!(notional(venue, 80_000.0, 0.5, 1.0), 40_000.0, "{} counts its sizes in coins", venue.id());
        }
    }

    #[test]
    fn the_window_slides_and_counts_each_side() {
        let doh = crate::dns::Doh::new().unwrap();
        let flow = Flow {
            pool: deadpool_postgres::Config { url: Some("postgres://nobody@127.0.0.1:1/none".into()), ..Default::default() }
                .create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls)
                .unwrap(),
            catalog: Arc::new(crate::catalog::Catalog::new("https://api.binance.com".into(), &doh).unwrap()),
            http: reqwest::Client::new(),
            doh,
            telegram: None,
            live: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            watches: Mutex::new(Vec::new()),
            said: Mutex::new(HashMap::new()),
            contracts: Mutex::new(HashMap::new()),
            admins: Vec::new(),
            coins: Mutex::new(Vec::new()),
            streams: Mutex::new(HashMap::new()),
            typical: Mutex::new(HashMap::new()),
        };
        let market = "binance-perp:BTCUSDT";
        let at = 1_800_000_000_000;
        let print = |price: f64, qty: f64, buy: bool| Print { symbol: "BTCUSDT", price, qty, buy, at };
        flow.record(market, &print(100.0, 30.0, true), 3_000.0, at);
        flow.record(market, &print(100.0, 10.0, false), 1_000.0, at + 1_000);
        {
            let live = flow.live.lock().unwrap();
            let entry = live.get(market).unwrap();
            assert_eq!((entry.buy, entry.sell, entry.trades), (3_000.0, 1_000.0, 2));
            assert_eq!(entry.delta(at), 2_000.0, "bought less sold");
            assert_eq!(entry.delta(at + 500), -1_000.0, "the window drops what fell out of it");
        }
        // A trade a minute later starts the minute afresh and slides the window.
        flow.record(market, &print(100.0, 1.0, true), 100.0, at + 70_000);
        let live = flow.live.lock().unwrap();
        let entry = live.get(market).unwrap();
        assert_eq!((entry.buy, entry.sell, entry.trades), (100.0, 0.0, 1), "a new minute counts from nothing");
        assert_eq!(entry.recent.len(), 1, "the older trades are out of the window");
    }

    #[test]
    fn a_crossing_reads_as_a_sentence() {
        let at = tradoor_core::time::parse_iso("2026-09-25T04:05:00Z").unwrap();
        let bought = flow_text("binance-perp:BTCUSDT", 5_400_000.0, 84_150.0, at, 8 * 60);
        assert_eq!(bought, "🟢 BTCUSDT perp — 5.4M net bought in a minute\nprice 84,150.00 — 2026-09-25 12:05 UTC+8");
        let sold = flow_text("binance-spot:BTCUSDT", -1_250_000.0, 84_000.0, at, 0);
        assert!(sold.starts_with("🔴 BTCUSDT spot — 1.2M net sold in a minute"), "{sold}");
    }
}
