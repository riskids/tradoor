//! Liquidations for a few tickers, collected on every perpetual exchange and
//! kept for a day, so the panel's 1H to 24H totals are whole the moment it opens.
//!
//! One WebSocket per exchange carries every ticker. Hits go down a channel to a
//! single writer, which inserts them in batches and trims the table to 24 hours.
//! None of it is on the request path: an exchange that is down only shows in the
//! status the API reports next to the totals.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{sleep, sleep_until, Instant};
use tokio_tungstenite::tungstenite::Message;

use crate::api::{ApiError, ApiResult, AppState};

pub const DAY_MS: i64 = 86_400_000;
/// The totals the panel shows, shortest first.
const WINDOWS: [(&str, i64); 4] = [("1h", 3_600_000), ("4h", 14_400_000), ("12h", 43_200_000), ("24h", DAY_MS)];
/// Seconds between reconnects, growing while an exchange stays unreachable.
const RETRY_SECS: [u64; 5] = [1, 3, 5, 10, 30];
/// Rows the panel's tape opens with.
const RECENT_ROWS: i64 = 200;
/// Hits waiting to be written before the writer stops waiting for the second.
const BATCH: usize = 500;

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// A ticker as the server takes it: `BTCUSDT`, the same spelling on every exchange.
pub fn is_ticker(s: &str) -> bool {
    (2..=30).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub venue: &'static str,
    pub symbol: String,
    /// Epoch milliseconds, as the exchange stamped it.
    pub time: i64,
    pub price: f64,
    pub qty: f64,
    /// A long position was closed out.
    pub long: bool,
}

/// An exchange's liquidation stream: where it is and how it speaks.
#[derive(Clone, Copy)]
pub struct Venue {
    pub id: &'static str,
    pub url: &'static str,
    subscribe: fn(&[String]) -> Vec<String>,
    parse: fn(&str) -> Vec<Hit>,
    /// A frame to send this often, for exchanges that close a socket nobody talks on.
    ping: Option<(&'static str, Duration)>,
    /// Reconnect when not a single frame has arrived for this long.
    silence: Duration,
}

pub const VENUES: [Venue; 2] = [
    // Binance pings every three minutes and drops a socket after a day.
    Venue { id: "binance-perp", url: "wss://fstream.binance.com/market/ws", subscribe: binance_subscribe, parse: parse_binance, ping: None, silence: Duration::from_secs(600) },
    // Bybit closes a socket that has not pinged in twenty seconds.
    Venue {
        id: "bybit-perp",
        url: "wss://stream.bybit.com/v5/public/linear",
        subscribe: bybit_subscribe,
        parse: parse_bybit,
        ping: Some((r#"{"op":"ping"}"#, Duration::from_secs(15))),
        silence: Duration::from_secs(60),
    },
];

fn binance_subscribe(tickers: &[String]) -> Vec<String> {
    let params: Vec<String> = tickers.iter().map(|t| format!("{}@forceOrder", t.to_lowercase())).collect();
    vec![json!({ "method": "SUBSCRIBE", "params": params, "id": 1 }).to_string()]
}

/// One topic a frame: Bybit refuses a whole frame over one topic it does not know.
fn bybit_subscribe(tickers: &[String]) -> Vec<String> {
    tickers.iter().map(|t| json!({ "op": "subscribe", "args": [format!("allLiquidation.{t}")] }).to_string()).collect()
}

/// Exchanges send numbers as strings; a number is taken as well.
fn num(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).filter(|n: &f64| n.is_finite())
}

/// Binance's `forceOrder`: one order a message. `p` is the limit the order was
/// pushed through the book at, a percent or more off the market; `ap` is what
/// it filled at, and is the price kept. A forced SELL closes a long.
pub fn parse_binance(text: &str) -> Vec<Hit> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    if v["e"] != "forceOrder" {
        return Vec::new();
    }
    let o = &v["o"];
    let price = num(&o["ap"]).filter(|p| *p > 0.0).or_else(|| num(&o["p"]));
    match (o["s"].as_str(), o["T"].as_i64(), price, num(&o["q"])) {
        (Some(symbol), Some(time), Some(price), Some(qty)) if price > 0.0 && qty > 0.0 => {
            vec![Hit { venue: "binance-perp", symbol: symbol.to_string(), time, price, qty, long: o["S"] == "SELL" }]
        }
        _ => Vec::new(),
    }
}

/// Bybit's `allLiquidation`: a batch a message. `S` is the side of the position
/// that was closed, so `Buy` is a long.
pub fn parse_bybit(text: &str) -> Vec<Hit> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    if !v["topic"].as_str().is_some_and(|t| t.starts_with("allLiquidation.")) {
        return Vec::new();
    }
    let Some(rows) = v["data"].as_array() else { return Vec::new() };
    rows.iter()
        .filter_map(|l| match (l["s"].as_str(), l["T"].as_i64(), num(&l["p"]), num(&l["v"])) {
            (Some(symbol), Some(time), Some(price), Some(qty)) if price > 0.0 && qty > 0.0 => {
                Some(Hit { venue: "bybit-perp", symbol: symbol.to_string(), time, price, qty, long: l["S"] == "Buy" })
            }
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueState {
    pub venue: &'static str,
    /// `connecting`, `connected` or `retrying`.
    pub state: &'static str,
    /// When it entered that state, epoch milliseconds.
    pub since: i64,
    pub error: Option<String>,
}

/// The running collection: what it follows, since when, and how each exchange is.
pub struct Collector {
    pub tickers: Vec<String>,
    pub started: i64,
    states: Mutex<Vec<VenueState>>,
}

impl Collector {
    /// Starts following `tickers` on every exchange. Returns at once; the work
    /// runs on the runtime for as long as the server does.
    pub fn start(pool: deadpool_postgres::Pool, tickers: Vec<String>) -> Arc<Self> {
        let started = now_ms();
        let states = VENUES.iter().map(|v| VenueState { venue: v.id, state: "connecting", since: started, error: None }).collect();
        let collector = Arc::new(Self { tickers, started, states: Mutex::new(states) });
        let (tx, rx) = mpsc::channel(8_192);
        tokio::spawn(write(pool, rx));
        for venue in VENUES {
            tokio::spawn(follow(venue, venue.url.to_string(), collector.clone(), tx.clone()));
        }
        tracing::info!(tickers = ?collector.tickers, "collecting liquidations");
        collector
    }

    pub fn states(&self) -> Vec<VenueState> {
        self.states.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn set(&self, venue: &str, state: &'static str, error: Option<String>) {
        let Ok(mut states) = self.states.lock() else { return };
        if let Some(s) = states.iter_mut().find(|s| s.venue == venue) {
            if s.state != state {
                s.since = now_ms();
            }
            s.state = state;
            s.error = error;
        }
    }
}

type Tx = mpsc::Sender<(String, Hit)>;

/// Keeps one exchange connected for as long as the writer is there to take hits.
async fn follow(venue: Venue, url: String, collector: Arc<Collector>, tx: Tx) {
    let mut attempt = 0;
    while !tx.is_closed() {
        collector.set(venue.id, "connecting", None);
        let result = session(&venue, &url, &collector, &tx).await;
        let error = match result {
            Ok(true) => {
                attempt = 0;
                "the exchange closed the connection".to_string()
            }
            Ok(false) => return,
            Err(e) => e.to_string(),
        };
        tracing::warn!(venue = venue.id, error = %error, "liquidation stream lost");
        collector.set(venue.id, "retrying", Some(error));
        sleep(Duration::from_secs(RETRY_SECS[attempt.min(RETRY_SECS.len() - 1)])).await;
        attempt += 1;
    }
}

/// One connection. `Ok(true)` when the exchange ended it and it is worth
/// reconnecting; `Ok(false)` when the writer has gone and so should we.
async fn session(venue: &Venue, url: &str, collector: &Collector, tx: &Tx) -> anyhow::Result<bool> {
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(url)).await.map_err(|_| anyhow::anyhow!("connecting timed out"))??;
    for frame in (venue.subscribe)(&collector.tickers) {
        ws.send(Message::text(frame)).await?;
    }
    collector.set(venue.id, "connected", None);

    let mut heard = Instant::now();
    let mut ping = venue.ping.map(|(_, every)| tokio::time::interval(every));
    loop {
        tokio::select! {
            frame = ws.next() => {
                let Some(frame) = frame else { return Ok(true) };
                heard = Instant::now();
                match frame? {
                    Message::Text(text) => {
                        for hit in (venue.parse)(&text) {
                            // The exchange spells the ticker as it was asked for.
                            let Some(ticker) = collector.tickers.iter().find(|t| **t == hit.symbol) else { continue };
                            if tx.send((ticker.clone(), hit)).await.is_err() {
                                return Ok(false);
                            }
                        }
                    }
                    Message::Close(_) => return Ok(true),
                    _ => {}
                }
            }
            () = tick(&mut ping) => {
                if let Some((frame, _)) = venue.ping {
                    ws.send(Message::text(frame)).await?;
                }
            }
            () = sleep_until(heard + venue.silence) => {
                anyhow::bail!("nothing heard for {} seconds", venue.silence.as_secs());
            }
        }
    }
}

async fn tick(interval: &mut Option<tokio::time::Interval>) {
    match interval {
        Some(i) => {
            i.tick().await;
        }
        None => std::future::pending().await,
    }
}

/// The one writer: hits in batches of up to a second, and a trim to 24 hours
/// every minute.
async fn write(pool: deadpool_postgres::Pool, mut rx: mpsc::Receiver<(String, Hit)>) {
    let mut batch = Vec::with_capacity(BATCH);
    let mut flush = tokio::time::interval(Duration::from_secs(1));
    let mut trim = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            got = rx.recv() => match got {
                Some(hit) => {
                    batch.push(hit);
                    if batch.len() >= BATCH {
                        insert(&pool, &mut batch).await;
                    }
                }
                None => {
                    insert(&pool, &mut batch).await;
                    return;
                }
            },
            _ = flush.tick() => insert(&pool, &mut batch).await,
            _ = trim.tick() => {
                if let Err(e) = trim_to_a_day(&pool).await {
                    tracing::error!(error = %e, "trimming liquidations");
                }
            }
        }
    }
}

/// Writes the batch in one statement. A failed batch is logged and dropped:
/// holding it while the database is away would only grow without bound.
async fn insert(pool: &deadpool_postgres::Pool, batch: &mut Vec<(String, Hit)>) {
    if batch.is_empty() {
        return;
    }
    let rows = std::mem::take(batch);
    let result = async {
        let client = pool.get().await.map_err(|e| e.to_string())?;
        let tickers: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
        let venues: Vec<&str> = rows.iter().map(|(_, h)| h.venue).collect();
        let symbols: Vec<&str> = rows.iter().map(|(_, h)| h.symbol.as_str()).collect();
        let times: Vec<i64> = rows.iter().map(|(_, h)| h.time).collect();
        let prices: Vec<f64> = rows.iter().map(|(_, h)| h.price).collect();
        let qtys: Vec<f64> = rows.iter().map(|(_, h)| h.qty).collect();
        let usds: Vec<f64> = rows.iter().map(|(_, h)| h.price * h.qty).collect();
        let longs: Vec<bool> = rows.iter().map(|(_, h)| h.long).collect();
        client
            .execute(
                "INSERT INTO liquidation (ticker, venue, symbol, time, price, qty, usd, long)
                 SELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::int8[], $5::float8[], $6::float8[], $7::float8[], $8::bool[])",
                &[&tickers, &venues, &symbols, &times, &prices, &qtys, &usds, &longs],
            )
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if let Err(e) = result {
        tracing::error!(error = %e, rows = rows.len(), "writing liquidations");
    }
}

async fn trim_to_a_day(pool: &deadpool_postgres::Pool) -> Result<(), String> {
    let client = pool.get().await.map_err(|e| e.to_string())?;
    let gone = client.execute("DELETE FROM liquidation WHERE time < $1", &[&(now_ms() - DAY_MS)]).await.map_err(|e| e.to_string())?;
    if gone > 0 {
        tracing::debug!(rows = gone, "trimmed liquidations older than a day");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowTotal {
    pub window: String,
    pub long_usd: f64,
    pub short_usd: f64,
    pub longs: i64,
    pub shorts: i64,
}

/// A ticker's four totals as of `now`, every exchange together.
async fn totals(client: &tokio_postgres::Client, ticker: &str, now: i64) -> Result<Vec<WindowTotal>, tokio_postgres::Error> {
    let rows = client
        .query(
            "SELECT w.name,
                    COALESCE(SUM(l.usd) FILTER (WHERE l.long), 0),
                    COALESCE(SUM(l.usd) FILTER (WHERE NOT l.long), 0),
                    COUNT(l.id) FILTER (WHERE l.long),
                    COUNT(l.id) FILTER (WHERE NOT l.long)
             FROM UNNEST($2::text[], $3::int8[]) AS w(name, span)
             LEFT JOIN liquidation l ON l.ticker = $1 AND l.time >= $4 - w.span AND l.time <= $4
             GROUP BY w.name, w.span
             ORDER BY w.span",
            &[&ticker, &WINDOWS.map(|w| w.0).to_vec(), &WINDOWS.map(|w| w.1).to_vec(), &now],
        )
        .await?;
    Ok(rows.iter().map(|r| WindowTotal { window: r.get(0), long_usd: r.get(1), short_usd: r.get(2), longs: r.get(3), shorts: r.get(4) }).collect())
}

/// `GET /api/liquidations/{ticker}`: the 1H, 4H, 12H and 24H totals across
/// every exchange, how each exchange's connection is, and the latest hits.
pub async fn stats(State(state): State<AppState>, Path(ticker): Path<String>) -> ApiResult<Json<Value>> {
    if !is_ticker(&ticker) {
        return Err(ApiError::bad_request("a ticker is letters and digits, like BTCUSDT"));
    }
    let Some(collector) = state.liquidations.as_ref().filter(|c| c.tickers.contains(&ticker)) else {
        let tickers = state.liquidations.as_ref().map(|c| c.tickers.clone()).unwrap_or_default();
        return Ok(Json(json!({ "ticker": ticker, "tracked": false, "tickers": tickers })));
    };

    let now = now_ms();
    let client = state.pool.get().await?;
    let windows = totals(&client, &ticker, now).await?;
    let oldest: Option<i64> = client.query_one("SELECT MIN(time) FROM liquidation WHERE ticker = $1", &[&ticker]).await?.get(0);
    let recent = client.query("SELECT venue, time, price, qty, usd, long FROM liquidation WHERE ticker = $1 ORDER BY time DESC, id DESC LIMIT $2", &[&ticker, &RECENT_ROWS]).await?;

    Ok(Json(json!({
        "ticker": ticker,
        "tracked": true,
        "now": now,
        // Hits are known from here on: the older of the first row kept and the
        // moment this server started listening.
        "coverageFrom": oldest.map_or(collector.started, |o| o.min(collector.started)),
        "windows": windows,
        "venues": collector.states(),
        "recent": recent.iter().map(|r| json!({
            "venue": r.get::<_, String>(0),
            "time": r.get::<_, i64>(1),
            "price": r.get::<_, f64>(2),
            "qty": r.get::<_, f64>(3),
            "usd": r.get::<_, f64>(4),
            "long": r.get::<_, bool>(5),
        })).collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    // Captured from fstream.binance.com, 2026-09-16.
    const FORCE_ORDER: &str = r#"{"e":"forceOrder","E":1789531696996,"o":{"s":"BTCUSDT","S":"BUY","o":"LIMIT","f":"IOC","q":"0.706","p":"76781.0","ap":"75363.5","X":"FILLED","l":"0.193","z":"0.706","T":1789531695983}}"#;

    #[test]
    fn binance_keeps_the_fill_price_and_a_forced_sell_is_a_long() {
        let hits = parse_binance(FORCE_ORDER);
        assert_eq!(hits, [Hit { venue: "binance-perp", symbol: "BTCUSDT".into(), time: 1789531695983, price: 75363.5, qty: 0.706, long: false }]);
        let sell = FORCE_ORDER.replace(r#""S":"BUY""#, r#""S":"SELL""#);
        assert!(parse_binance(&sell)[0].long, "a forced sell closes a long");
        // An order not filled yet has no average price; its limit stands in.
        let unfilled = FORCE_ORDER.replace(r#""ap":"75363.5""#, r#""ap":"0""#);
        assert_eq!(parse_binance(&unfilled)[0].price, 76781.0);
        assert!(parse_binance(r#"{"result":null,"id":1}"#).is_empty(), "a subscribe answer is not a hit");
    }

    #[test]
    fn bybit_reads_a_batch_and_buy_is_a_long() {
        let hits = parse_bybit(r#"{"topic":"allLiquidation.ETHUSDT","type":"snapshot","ts":1789532479754,"data":[{"T":1789532479393,"s":"ETHUSDT","S":"Buy","v":"8.5","p":"2401.3"},{"T":1789532479400,"s":"ETHUSDT","S":"Sell","v":"0","p":"2401.0"},{"T":1789532479500,"s":"ETHUSDT","S":"Sell","v":"1.25","p":"2400.9"}]}"#);
        assert_eq!(hits.len(), 2, "a zero-size row is not a hit");
        assert!(hits[0].long && !hits[1].long);
        assert_eq!((hits[1].price, hits[1].qty, hits[1].time), (2400.9, 1.25, 1789532479500));
        assert!(parse_bybit(r#"{"success":true,"ret_msg":"pong","op":"ping"}"#).is_empty());
    }

    #[test]
    fn subscriptions_ask_for_every_ticker() {
        let tickers = ["BTCUSDT".to_string(), "SOLUSDT".to_string()];
        assert_eq!(binance_subscribe(&tickers), [r#"{"id":1,"method":"SUBSCRIBE","params":["btcusdt@forceOrder","solusdt@forceOrder"]}"#]);
        assert_eq!(bybit_subscribe(&tickers).len(), 2, "one Bybit topic a frame");
        assert!(bybit_subscribe(&tickers)[1].contains("allLiquidation.SOLUSDT"));
    }

    #[test]
    fn tickers_are_plain_symbols() {
        assert!(is_ticker("BTCUSDT") && is_ticker("1000PEPEUSDT"));
        assert!(!is_ticker("btcusdt") && !is_ticker("BTC-USDT") && !is_ticker("B") && !is_ticker("DROP TABLE"));
    }

    /// The write, the trim and the totals against a real database — the SQL and
    /// the array bindings are what this checks. Needs DATABASE_URL with the
    /// schema applied; run with `cargo test -p tradoor-server -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a Postgres at DATABASE_URL"]
    async fn hits_are_written_totalled_and_trimmed_to_a_day() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = deadpool_postgres::Config { url: Some(url), ..Default::default() }.create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls).unwrap();
        const T: &str = "ZZTESTUSDT";
        let client = pool.get().await.unwrap();
        client.execute("DELETE FROM liquidation WHERE ticker = $1", &[&T]).await.unwrap();

        let now = now_ms();
        let hit = |venue: &'static str, ago: i64, price: f64, qty: f64, long: bool| (T.to_string(), Hit { venue, symbol: T.into(), time: now - ago, price, qty, long });
        let mut batch = vec![
            hit("binance-perp", 10 * 60_000, 100.0, 2.0, true),     // $200 long, 10 minutes ago
            hit("bybit-perp", 2 * 3_600_000, 50.0, 3.0, false),     // $150 short, 2 hours ago
            hit("bybit-perp", 11 * 3_600_000, 10.0, 1.0, true),     // $10 long, 11 hours ago
            hit("binance-perp", 25 * 3_600_000, 1000.0, 1.0, true), // a day and an hour ago
        ];
        insert(&pool, &mut batch).await;
        assert!(batch.is_empty(), "the batch is taken");

        let before = totals(&client, T, now).await.unwrap();
        assert_eq!(before.iter().map(|w| w.window.as_str()).collect::<Vec<_>>(), ["1h", "4h", "12h", "24h"]);
        assert_eq!((before[0].long_usd, before[0].short_usd, before[0].longs), (200.0, 0.0, 1), "1h: the recent long only");
        assert_eq!((before[1].long_usd, before[1].short_usd, before[1].shorts), (200.0, 150.0, 1), "4h: both exchanges together");
        assert_eq!(before[2].long_usd, 210.0, "12h");
        assert_eq!(before[3].long_usd, 210.0, "24h leaves out the day-old hit");

        trim_to_a_day(&pool).await.unwrap();
        let left: i64 = client.query_one("SELECT COUNT(*) FROM liquidation WHERE ticker = $1", &[&T]).await.unwrap().get(0);
        client.execute("DELETE FROM liquidation WHERE ticker = $1", &[&T]).await.unwrap();
        assert_eq!(left, 3, "the trim removed only what is older than a day");
    }

    /// A whole session against a stand-in exchange: it subscribes, passes on a
    /// hit for a followed ticker, ignores one for another, and reports the
    /// exchange closing the socket.
    #[tokio::test]
    async fn a_session_subscribes_and_passes_hits_on() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let exchange = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let subscribe = ws.next().await.unwrap().unwrap().into_text().unwrap();
            ws.send(Message::text(r#"{"result":null,"id":1}"#)).await.unwrap();
            ws.send(Message::text(FORCE_ORDER.replace("BTCUSDT", "DOGEUSDT"))).await.unwrap();
            ws.send(Message::text(FORCE_ORDER)).await.unwrap();
            ws.close(None).await.unwrap();
            subscribe.to_string()
        });

        let collector = Collector { tickers: vec!["BTCUSDT".into()], started: now_ms(), states: Mutex::new(vec![VenueState { venue: "binance-perp", state: "connecting", since: 0, error: None }]) };
        let (tx, mut rx) = mpsc::channel(8);
        let reconnect = session(&VENUES[0], &url, &collector, &tx).await.unwrap();

        assert!(reconnect, "the exchange closed it, so it is worth reconnecting");
        assert_eq!(exchange.await.unwrap(), r#"{"id":1,"method":"SUBSCRIBE","params":["btcusdt@forceOrder"]}"#);
        let (ticker, hit) = rx.try_recv().expect("the BTCUSDT hit came through");
        assert_eq!((ticker.as_str(), hit.price), ("BTCUSDT", 75363.5));
        assert!(rx.try_recv().is_err(), "DOGEUSDT is not followed");
        assert_eq!(collector.states()[0].state, "connected");
    }
}
