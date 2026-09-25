//! Watching the exchanges for alerts, so one fires with no browser open.
//!
//! The page still watches the prices it holds anyway — that is what makes an
//! alert feel instant — but a closed browser watches nothing, and the alert
//! nobody is looking at is the one worth keeping. So every market with an
//! active alert is followed here on its exchange's public trade stream, and a
//! price that meets an alert's condition takes the same two statements the
//! API's own trigger takes: the alert stops, the log gains a line. Whichever
//! side notices first wins; the other finds it already stopped and writes
//! nothing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::watch;
use tokio::time::{sleep, sleep_until, Instant};
use tokio_tungstenite::tungstenite::Message;
use tradoor_store::now_ms;
use tradoor_venues::{MarketRef, Venue};

use crate::alerts;
use crate::dns::Doh;
use crate::telegram::{self, Telegram};

/// The venues a chart can show, and so the ones an alert can name.
const VENUES: [Venue; 4] = [Venue::BinanceSpot, Venue::BinancePerp, Venue::BybitSpot, Venue::BybitPerp];
/// How often the alerts to watch are read again.
const REFRESH: Duration = Duration::from_secs(10);
const RETRY_SECS: [u64; 5] = [1, 3, 5, 10, 30];
/// Nothing at all from an exchange for this long means the socket is gone.
const SILENCE: Duration = Duration::from_secs(90);
/// Answering but carrying no trade for this long means the address is a dead
/// end — one of Binance's two took our subscription and then sent nothing — so
/// the next one is tried. A market that truly is this quiet loses only a
/// reconnection by it.
const NO_TRADES: Duration = Duration::from_secs(150);
/// Bybit closes a socket that says nothing for 20 seconds.
const PING: Duration = Duration::from_secs(15);
/// Markets followed on one exchange. Past this the page is on its own: a server
/// holding thousands of alerts should not open a subscription for each.
const MAX_SYMBOLS: usize = 200;

/// An alert as the watcher needs it: who it belongs to, and when it fires.
#[derive(Clone, Debug, PartialEq)]
pub struct Watched {
    pub id: i64,
    pub owner: String,
    pub market: String,
    pub venue: Venue,
    pub symbol: String,
    pub condition: String,
    pub price: f64,
}

/// Whether a move from `prev` to `cur` meets `condition` at `level`: the rule
/// the page follows, in TradingView's wording. A cross needs a price on each
/// side, so the first price only arms it; reaching the level exactly counts.
pub fn fires(condition: &str, level: f64, prev: Option<f64>, cur: f64) -> bool {
    match condition {
        "greater" => cur > level,
        "less" => cur < level,
        "crossing_up" => prev.is_some_and(|p| p < level) && cur >= level,
        "crossing_down" => prev.is_some_and(|p| p > level) && cur <= level,
        "crossing" => prev.is_some_and(|p| (p < level && cur >= level) || (p > level && cur <= level)),
        _ => false,
    }
}

/// The public streams each venue's trades come on, best first. A venue with
/// more than one is tried in turn: an address that answers but then says
/// nothing is as useless as one that refuses, and which is which moves with the
/// network — the page carries the same lists for the same reason.
fn stream_urls(venue: Venue) -> &'static [&'static str] {
    match venue {
        Venue::BinanceSpot => &["wss://stream.binance.com:9443/ws", "wss://data-stream.binance.vision/ws"],
        Venue::BinancePerp => &["wss://fstream.binance.com/market/ws", "wss://fstream.binance.com/ws"],
        Venue::BybitSpot => &["wss://stream.bybit.com/v5/public/spot"],
        Venue::BybitPerp => &["wss://stream.bybit.com/v5/public/linear"],
        _ => &[],
    }
}

fn subscribe(venue: Venue, symbols: &[String]) -> String {
    match venue {
        Venue::BybitSpot | Venue::BybitPerp => {
            let args: Vec<String> = symbols.iter().map(|s| format!("\"publicTrade.{s}\"")).collect();
            format!("{{\"op\":\"subscribe\",\"args\":[{}]}}", args.join(","))
        }
        _ => {
            let params: Vec<String> = symbols.iter().map(|s| format!("\"{}@aggTrade\"", s.to_lowercase())).collect();
            format!("{{\"method\":\"SUBSCRIBE\",\"params\":[{}],\"id\":1}}", params.join(","))
        }
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        _ => v.as_f64(),
    }
    .filter(|p: &f64| p.is_finite() && *p > 0.0)
}

/// The trades in one frame, as `(symbol, price)`. Anything else — a
/// subscription answer, a heartbeat — carries no trade and is skipped.
pub fn parse_trades(venue: Venue, text: &str) -> Vec<(String, f64)> {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    match venue {
        Venue::BybitSpot | Venue::BybitPerp => {
            if !v["topic"].as_str().is_some_and(|t| t.starts_with("publicTrade.")) {
                return Vec::new();
            }
            v["data"].as_array().map(|rows| rows.iter().filter_map(|r| Some((r["s"].as_str()?.to_string(), num(&r["p"])?))).collect()).unwrap_or_default()
        }
        _ => {
            if v["e"] != "aggTrade" {
                return Vec::new();
            }
            v["s"].as_str().zip(num(&v["p"])).map(|(s, p)| vec![(s.to_string(), p)]).unwrap_or_default()
        }
    }
}

/// Follow every market with an active alert, for as long as the server runs.
pub fn start(pool: deadpool_postgres::Pool, doh: Arc<Doh>, telegram: Option<Arc<Telegram>>) {
    let (tx, rx) = watch::channel(Arc::new(Vec::<Watched>::new()));
    tokio::spawn(refresh(pool.clone(), tx));
    for venue in VENUES {
        tokio::spawn(follow(venue, pool.clone(), doh.clone(), telegram.clone(), rx.clone()));
    }
    tracing::info!("watching alerts");
}

/// The active alerts, read again every few seconds: one added on a phone is
/// watched here within that, without the page telling the server anything.
async fn refresh(pool: deadpool_postgres::Pool, tx: watch::Sender<Arc<Vec<Watched>>>) {
    loop {
        match read(&pool).await {
            Ok(list) => {
                tx.send_if_modified(|current| {
                    let same = current.as_slice() == list.as_slice();
                    if !same {
                        *current = Arc::new(list);
                    }
                    !same
                });
            }
            Err(e) => tracing::warn!(error = %e, "reading alerts to watch"),
        }
        if tx.is_closed() {
            return;
        }
        sleep(REFRESH).await;
    }
}

async fn read(pool: &deadpool_postgres::Pool) -> anyhow::Result<Vec<Watched>> {
    let client = pool.get().await?;
    let rows = client.query("SELECT id, owner, market, condition, price FROM alert WHERE active ORDER BY id", &[]).await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            let market: String = r.get(2);
            let m = MarketRef::parse(&market)?;
            (!stream_urls(m.venue).is_empty()).then_some(())?;
            Some(Watched { id: r.get(0), owner: r.get(1), market, venue: m.venue, symbol: m.symbol, condition: r.get(3), price: r.get(4) })
        })
        .collect())
}

/// The symbols one venue is asked for, in a settled order so an unchanged set
/// never looks like a change.
fn symbols_of(venue: Venue, alerts: &[Watched]) -> Vec<String> {
    let mut symbols: Vec<String> = alerts.iter().filter(|a| a.venue == venue).map(|a| a.symbol.clone()).collect();
    symbols.sort_unstable();
    symbols.dedup();
    symbols.truncate(MAX_SYMBOLS);
    symbols
}

/// Keeps one exchange connected while any alert names it.
async fn follow(venue: Venue, pool: deadpool_postgres::Pool, doh: Arc<Doh>, telegram: Option<Arc<Telegram>>, mut alerts: watch::Receiver<Arc<Vec<Watched>>>) {
    let urls = stream_urls(venue);
    if urls.is_empty() {
        return;
    }
    let (mut attempt, mut at) = (0, 0);
    loop {
        let url = urls[at % urls.len()];
        let symbols = symbols_of(venue, &alerts.borrow_and_update().clone());
        if symbols.is_empty() {
            // Nothing to watch here: wait until an alert names this exchange.
            if alerts.changed().await.is_err() {
                return;
            }
            continue;
        }
        match session(venue, url, &symbols, &pool, &doh, telegram.as_ref(), &mut alerts).await {
            Ok(()) => attempt = 0,
            Err(e) => {
                tracing::warn!(venue = venue.id(), url, error = %e, "alert stream lost");
                sleep(Duration::from_secs(RETRY_SECS[attempt.min(RETRY_SECS.len() - 1)])).await;
                (attempt, at) = (attempt + 1, at + 1);
            }
        }
    }
}

/// One connection, until the exchange ends it or the markets to watch change.
async fn session(
    venue: Venue,
    url: &str,
    symbols: &[String],
    pool: &deadpool_postgres::Pool,
    doh: &Doh,
    telegram: Option<&Arc<Telegram>>,
    alerts: &mut watch::Receiver<Arc<Vec<Watched>>>,
) -> anyhow::Result<()> {
    let connect = doh.connect_ws(url);
    let mut ws = tokio::time::timeout(Duration::from_secs(15), connect).await.map_err(|_| anyhow::anyhow!("connecting timed out"))??;
    ws.send(Message::text(subscribe(venue, symbols))).await?;
    tracing::info!(venue = venue.id(), markets = symbols.len(), "watching alert prices");

    let (mut heard, mut traded) = (Instant::now(), Instant::now());
    let mut ping = tokio::time::interval(PING);
    ping.reset();
    // The price before the last, per symbol: a cross needs a price on each side.
    let mut previous: HashMap<String, f64> = HashMap::new();
    // Alerts already fired here, so a level that stays met is reported once.
    let mut fired: HashSet<i64> = HashSet::new();
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
                for (symbol, price) in parse_trades(venue, &text) {
                    traded = Instant::now();
                    let market = format!("{}:{}", venue.id(), symbol);
                    let prev = previous.insert(symbol, price);
                    let due: Vec<Watched> = alerts
                        .borrow()
                        .iter()
                        .filter(|a| a.market == market && !fired.contains(&a.id) && fires(&a.condition, a.price, prev, price))
                        .cloned()
                        .collect();
                    for alert in due {
                        fired.insert(alert.id);
                        if let Err(e) = trigger(pool, telegram, &alert, price).await {
                            tracing::warn!(alert = alert.id, error = %e, "recording an alert trigger");
                        }
                    }
                }
            }
            changed = alerts.changed() => {
                changed?;
                let list = alerts.borrow_and_update().clone();
                if symbols_of(venue, &list) != symbols {
                    return Ok(());
                }
                // An alert that is gone from the list has stopped: forget it, so
                // the same id made active again is watched afresh.
                fired.retain(|id| list.iter().any(|a| a.id == *id));
            }
            _ = ping.tick() => {
                if matches!(venue, Venue::BybitSpot | Venue::BybitPerp) {
                    ws.send(Message::text("{\"op\":\"ping\"}")).await?;
                }
            }
            () = sleep_until(heard + SILENCE) => anyhow::bail!("nothing heard for {} seconds", SILENCE.as_secs()),
            () = sleep_until(traded + NO_TRADES) => anyhow::bail!("no trade in {} seconds", NO_TRADES.as_secs()),
        }
    }
}

async fn trigger(pool: &deadpool_postgres::Pool, telegram: Option<&Arc<Telegram>>, alert: &Watched, value: f64) -> anyhow::Result<()> {
    let client = pool.get().await?;
    let now = now_ms();
    let Some((fired, _)) = alerts::record_trigger(&client, &alert.owner, alert.id, value, now).await? else { return Ok(()) };
    tracing::info!(alert = alert.id, market = %alert.market, condition = %alert.condition, level = alert.price, value, "alert fired");
    telegram::notify_fired(telegram, &alert.owner, &fired, value, now).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_fires_the_way_the_page_fires_it() {
        assert!(fires("greater", 100.0, None, 101.0) && !fires("greater", 100.0, None, 100.0));
        assert!(fires("less", 100.0, Some(101.0), 99.0) && !fires("less", 100.0, None, 100.0));
        assert!(!fires("crossing_up", 100.0, None, 101.0), "the first price only arms a cross");
        assert!(fires("crossing_up", 100.0, Some(99.0), 100.0), "reaching the level counts");
        assert!(!fires("crossing_up", 100.0, Some(101.0), 102.0));
        assert!(fires("crossing_down", 100.0, Some(101.0), 100.0) && !fires("crossing_down", 100.0, Some(99.0), 98.0));
        assert!(fires("crossing", 100.0, Some(99.0), 101.0) && fires("crossing", 100.0, Some(101.0), 99.0));
        assert!(!fires("crossing", 100.0, Some(101.0), 102.0) && !fires("nonsense", 100.0, Some(1.0), 200.0));
    }

    #[test]
    fn trades_are_read_from_each_exchange_and_nothing_else_is() {
        let binance = r#"{"e":"aggTrade","E":1,"s":"BTCUSDT","a":1,"p":"84250.10","q":"0.01"}"#;
        assert_eq!(parse_trades(Venue::BinancePerp, binance), [("BTCUSDT".to_string(), 84_250.10)]);
        assert!(parse_trades(Venue::BinancePerp, r#"{"result":null,"id":1}"#).is_empty(), "the subscription answer is not a trade");
        assert!(parse_trades(Venue::BinancePerp, r#"{"e":"kline","s":"BTCUSDT","k":{"c":"1"}}"#).is_empty());

        let bybit = r#"{"topic":"publicTrade.BTCUSDT","type":"snapshot","data":[{"s":"BTCUSDT","p":"84251.5","v":"0.1"},{"s":"BTCUSDT","p":"84252","v":"0.2"}]}"#;
        assert_eq!(parse_trades(Venue::BybitSpot, bybit), [("BTCUSDT".to_string(), 84_251.5), ("BTCUSDT".to_string(), 84_252.0)]);
        assert!(parse_trades(Venue::BybitSpot, r#"{"success":true,"op":"subscribe"}"#).is_empty());
        assert!(parse_trades(Venue::BybitSpot, "not json").is_empty());
    }

    #[test]
    fn each_exchange_is_asked_in_its_own_words() {
        let symbols = ["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        assert_eq!(subscribe(Venue::BinanceSpot, &symbols), r#"{"method":"SUBSCRIBE","params":["btcusdt@aggTrade","ethusdt@aggTrade"],"id":1}"#);
        assert_eq!(subscribe(Venue::BybitPerp, &symbols), r#"{"op":"subscribe","args":["publicTrade.BTCUSDT","publicTrade.ETHUSDT"]}"#);
        assert!(stream_urls(Venue::OkxPerp).is_empty(), "a venue with no chart has no alerts to watch");
        assert!(stream_urls(Venue::BinancePerp).len() > 1, "an address that goes quiet is retried on the next one");
    }

    /// Needs a Postgres at DATABASE_URL with the schema applied; run with
    /// `cargo test -p tradoor-server -- --ignored`.
    #[tokio::test]
    #[ignore = "needs a Postgres at DATABASE_URL"]
    async fn an_alert_fires_once_however_many_watchers_report_it() {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let pool = deadpool_postgres::Config { url: Some(url), ..Default::default() }.create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls).unwrap();
        const OWNER: &str = "zz-test-watcher";
        let client = pool.get().await.unwrap();
        client.execute("DELETE FROM alert_log WHERE owner = $1", &[&OWNER]).await.unwrap();
        client.execute("DELETE FROM alert WHERE owner = $1", &[&OWNER]).await.unwrap();

        let now = now_ms();
        let id: i64 = client
            .query_one("INSERT INTO alert (owner, market, condition, price, created_at) VALUES ($1, 'binance-perp:BTCUSDT', 'greater', 100, $2) RETURNING id", &[&OWNER, &now])
            .await
            .unwrap()
            .get(0);

        let first = alerts::record_trigger(&client, OWNER, id, 101.0, now).await.unwrap();
        assert!(first.is_some(), "the price met the level");
        let again = alerts::record_trigger(&client, OWNER, id, 102.0, now + 1).await.unwrap();
        assert!(again.is_none(), "the page reporting the same cross writes nothing");

        let row = client.query_one("SELECT active, trigger_price FROM alert WHERE id = $1", &[&id]).await.unwrap();
        assert_eq!((row.get::<_, bool>(0), row.get::<_, f64>(1)), (false, 101.0));
        let logs: i64 = client.query_one("SELECT count(*) FROM alert_log WHERE owner = $1", &[&OWNER]).await.unwrap().get(0);
        assert_eq!(logs, 1, "one line, from the watcher that got there first");

        client.execute("DELETE FROM alert_log WHERE owner = $1", &[&OWNER]).await.unwrap();
        client.execute("DELETE FROM alert WHERE owner = $1", &[&OWNER]).await.unwrap();
    }

    #[test]
    fn a_venue_is_asked_for_each_of_its_markets_once() {
        let alert = |id: i64, venue: Venue, symbol: &str| Watched {
            id,
            owner: "o".into(),
            market: format!("{}:{symbol}", venue.id()),
            venue,
            symbol: symbol.into(),
            condition: "crossing".into(),
            price: 1.0,
        };
        let list = [alert(1, Venue::BinancePerp, "BTCUSDT"), alert(2, Venue::BybitPerp, "SOLUSDT"), alert(3, Venue::BinancePerp, "BTCUSDT"), alert(4, Venue::BinancePerp, "ETHUSDT")];
        assert_eq!(symbols_of(Venue::BinancePerp, &list), ["BTCUSDT", "ETHUSDT"]);
        assert_eq!(symbols_of(Venue::BybitPerp, &list), ["SOLUSDT"]);
        assert!(symbols_of(Venue::BybitSpot, &list).is_empty());
    }
}
