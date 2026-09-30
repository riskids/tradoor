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

/// The markets this server follows, in the order the panel shows them.
pub fn markets() -> Vec<String> {
    MARKETS.iter().map(|(market, _)| (*market).to_string()).collect()
}

fn default_threshold(market: &str) -> f64 {
    MARKETS.iter().find(|(m, _)| *m == market).map_or(1_000_000.0, |(_, t)| *t)
}

/// What a trade was worth. Everywhere but OKX's swaps a size is coins, and the
/// money is price times size; there it is contracts of a hundredth of a coin,
/// so a print of fifty is half a coin and not fifty of them.
fn notional(venue: Venue, price: f64, qty: f64, contract: f64) -> f64 {
    if venue == Venue::OkxPerp { price * qty * contract } else { price * qty }
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
}

impl Flow {
    pub fn start(pool: deadpool_postgres::Pool, doh: Arc<Doh>, catalog: Arc<Catalog>, telegram: Option<Arc<Telegram>>) -> Arc<Self> {
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
        });
        for market in markets() {
            tokio::spawn(follow(market, flow.clone()));
        }
        tokio::spawn(keep(flow.clone()));
        tokio::spawn(weigh(flow.clone()));
        tracing::info!(markets = ?markets(), "watching flow");
        flow
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

    /// The flow of the last minute on each market, with the last price.
    pub fn now(&self) -> Vec<(String, f64, f64)> {
        let since = now_ms() - WINDOW_MS;
        let live = self.live.lock().ok();
        markets()
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

/// Writes the minutes down, trims what is older than two days, and reads back
/// what the readers asked for.
async fn keep(flow: Arc<Flow>) {
    let mut trimmed = Instant::now();
    loop {
        sleep(FLUSH).await;
        if let Err(e) = write_minutes(&flow).await {
            tracing::warn!(error = %e, "writing flow minutes");
        }
        match read_watches(&flow.pool).await {
            Ok(watches) => {
                if let Ok(mut held) = flow.watches.lock() {
                    *held = watches;
                }
            }
            Err(e) => tracing::warn!(error = %e, "reading flow watches"),
        }
        if trimmed.elapsed() >= Duration::from_secs(600) {
            trimmed = Instant::now();
            if let Err(e) = trim(&flow.pool).await {
                tracing::warn!(error = %e, "trimming flow");
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

/// `GET /api/flow`: every market's flow now and over the last day, this
/// reader's settings, and the crossings they were told about.
pub async fn state(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let Some(flow) = state.flow.as_ref() else {
        return Ok(Json(json!({ "watching": false, "markets": [], "watches": [], "events": [] })));
    };
    let client = state.pool.get().await?;
    let now = now_ms();
    let sums = client
        .query(
            "SELECT market,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $1) AS m5,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $2) AS m15,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $3) AS h1,
                    sum(buy_usd - sell_usd) FILTER (WHERE minute >= $4) AS h4,
                    sum(buy_usd - sell_usd) AS day
             FROM flow_minute WHERE minute >= $5 GROUP BY market",
            &[&(now - 5 * 60_000), &(now - 15 * 60_000), &(now - 3_600_000), &(now - 4 * 3_600_000), &(now - 24 * 3_600_000)],
        )
        .await?;
    let sum = |market: &str, at: usize| sums.iter().find(|r| r.get::<_, String>(0) == market).and_then(|r| r.get::<_, Option<f64>>(at)).unwrap_or(0.0);
    let watches = client.query("SELECT market, threshold_usd, notify FROM flow_watch WHERE owner = $1", &[&owner]).await?;
    let events = client.query("SELECT id, market, at, delta_usd, window_ms, price FROM flow_event WHERE owner = $1 ORDER BY at DESC, id DESC LIMIT $2", &[&owner, &EVENTS_KEPT]).await?;
    let live = flow.now();
    let markets = live
        .iter()
        .map(|(market, minute, price)| {
            let watch = watches.iter().find(|r| r.get::<_, String>(0) == *market);
            json!({
                "market": market,
                "price": price,
                "minute": minute,
                "m5": sum(market, 1),
                "m15": sum(market, 2),
                "h1": sum(market, 3),
                "h4": sum(market, 4),
                "day": sum(market, 5),
                "thresholdUsd": watch.map_or_else(|| default_threshold(market), |r| r.get::<_, f64>(1)),
                "notify": watch.is_some_and(|r| r.get::<_, bool>(2)),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "watching": true, "windowMs": WINDOW_MS, "markets": markets, "events": events.iter().map(row_json).collect::<Vec<_>>() })))
}

/// `GET /api/flow/minutes?market=…`: the minutes of the last two days whose
/// flow ran harder than this reader's threshold, for the chart to mark.
pub async fn minutes(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<MinutesQuery>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    if !markets().contains(&q.market) {
        return Ok(Json(json!({ "market": q.market, "thresholdUsd": 0.0, "minutes": [] })));
    }
    let client = state.pool.get().await?;
    let watch = client.query_opt("SELECT threshold_usd FROM flow_watch WHERE owner = $1 AND market = $2", &[&owner, &q.market]).await?;
    let threshold = watch.map_or_else(|| default_threshold(&q.market), |r| r.get::<_, f64>(0));
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
    if !markets().contains(&market) {
        return Err(ApiError::bad_request("this server does not follow that market's flow"));
    }
    let threshold = body["thresholdUsd"].as_f64().unwrap_or_else(|| default_threshold(&market));
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
        assert_eq!(markets().len(), 11);
        assert!(markets().contains(&"binance-perp:BTCUSDT".to_string()) && markets().contains(&"hyperliquid-perp:BTCUSD".to_string()));
        assert!(markets().iter().all(|m| MarketRef::parse(m).is_some_and(|r| !trades::stream_urls(r.venue).is_empty())), "every one of them has a stream to read");
        assert!(default_threshold("binance-perp:BTCUSDT") > default_threshold("binance-spot:BTCUSDT"), "a perpetual turns over more, so it takes more to speak");
        assert_eq!(default_threshold("nowhere:BTCUSDT"), 1_000_000.0);
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
