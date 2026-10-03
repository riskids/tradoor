//! The Signal panel's screener. A minute after each four-hour bar closes, the
//! two hundred coins with the most turnover on Binance's and Bybit's USDT
//! perpetuals are read — five hundred closed bars each — and searched for three
//! drives either way (`drives`). A pattern is kept with the last scan that
//! still saw it: the panel lists what the latest scan saw, and the readers who
//! asked are told on Telegram, once, when a pattern turns.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::Json;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tradoor_core::format::format_price;
use tradoor_core::{Candle, Interval};
use tradoor_store::now_ms;
use tradoor_venues::{binance, bybit, Exchange, MarketRef};

use crate::api::{owner, ApiError, ApiResult, AppState};
use crate::catalog::Catalog;
use crate::dns::Doh;
use crate::drives::{self, Found, Side};
use crate::telegram::{self, Telegram};

/// Coins screened, by a day's turnover.
const COINS: usize = 200;
/// Closed bars read per coin: enough for forty days of drives and their warm-up.
const BARS: u32 = 500;
const STEP: i64 = 4 * 3_600_000;
/// How long after a bar closes it is read: the exchanges take a moment to settle it.
const SETTLE_MS: i64 = 60_000;
/// Requests in flight at once.
const AT_ONCE: usize = 4;
/// Patterns and scans kept, for the record of how they went.
const KEPT_MS: i64 = 90 * 24 * 3_600_000;
const KIND: &str = "three-drives";
/// Patterns named in one Telegram message; the rest are in the panel.
const TOLD_AT_MOST: usize = 12;

struct Screener {
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
    catalog: Arc<Catalog>,
    telegram: Option<Arc<Telegram>>,
}

pub fn start(pool: deadpool_postgres::Pool, doh: &Arc<Doh>, catalog: Arc<Catalog>, telegram: Option<Arc<Telegram>>) {
    let http = reqwest::Client::builder().dns_resolver(doh.resolver()).timeout(Duration::from_secs(20)).build().unwrap_or_default();
    tokio::spawn(Arc::new(Screener { pool, http, catalog, telegram }).run());
}

/// The close of the newest bar that has settled: what a scan now reads up to.
fn settled(now: i64) -> i64 {
    (now - SETTLE_MS).div_euclid(STEP) * STEP
}

impl Screener {
    async fn run(self: Arc<Self>) {
        // A moment after a start for the catalogue and the database.
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            let bar = settled(now_ms());
            // A restart within the same four hours does not scan again.
            let done = match self.pool.get().await {
                Ok(client) => client.query_opt("SELECT max(bar) FROM signal_scan", &[]).await.ok().flatten().and_then(|r| r.get::<_, Option<i64>>(0)),
                Err(_) => None,
            };
            if done.is_none_or(|d| d < bar) {
                if let Err(e) = self.scan(bar).await {
                    tracing::warn!(error = %e, "signal scan");
                }
            }
            let wait = bar + STEP + SETTLE_MS - now_ms();
            tokio::time::sleep(Duration::from_millis(wait.max(1_000) as u64)).await;
        }
    }

    async fn scan(&self, bar: i64) -> anyhow::Result<()> {
        let started = now_ms();
        let markets = self.catalog.perp_leaders(COINS).await;
        if markets.is_empty() {
            bail!("the catalogue lists no perpetuals yet");
        }
        let results: Vec<(String, anyhow::Result<Vec<Candle>>)> =
            futures_util::stream::iter(markets).map(|market| async move { (market.clone(), self.bars(&market, bar).await) }).buffer_unordered(AT_ONCE).collect().await;
        let mut read = 0;
        let mut found = Vec::new();
        for (market, bars) in results {
            match bars {
                Ok(bars) => {
                    read += 1;
                    let price = bars.last().map_or(0.0, |c| c.close);
                    for side in [Side::Bear, Side::Bull] {
                        if let Some(f) = drives::find(&bars, side) {
                            found.push((market.clone(), f, price));
                        }
                    }
                }
                Err(e) => tracing::debug!(%market, error = %e, "signal scan: bars not read"),
            }
        }
        self.keep(bar, read, &found).await?;
        tracing::info!(markets = read, found = found.len(), secs = (now_ms() - started) / 1000, "signal scan");
        self.tell(bar).await
    }

    /// A market's closed four-hour bars up to `bar`, oldest first.
    async fn bars(&self, market: &str, bar: i64) -> anyhow::Result<Vec<Candle>> {
        let m = MarketRef::parse(market).ok_or_else(|| anyhow!("{market} is not a market"))?;
        let exchange = m.venue.exchange();
        let url = match exchange {
            Exchange::Binance => binance::klines_url(binance::PERP_REST, &m, Interval::H4, None, None, BARS),
            Exchange::Bybit => bybit::klines_url(bybit::REST, &m, Interval::H4, None, None, BARS),
            _ => bail!("only Binance and Bybit are screened"),
        };
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            bail!("{status} from {url}");
        }
        let klines = if exchange == Exchange::Binance { binance::parse_klines(&body)? } else { bybit::parse_klines(&body)? };
        klines
            .iter()
            .filter(|k| k.open_time + STEP <= bar)
            .map(|k| {
                Ok(Candle {
                    open_time: k.open_time,
                    open: k.open.parse()?,
                    high: k.high.parse()?,
                    low: k.low.parse()?,
                    close: k.close.parse()?,
                    volume: k.volume.parse()?,
                    quote_volume: None,
                })
            })
            .collect()
    }

    /// The scan and what it found, a pattern seen before keeping the bar it first turned on.
    async fn keep(&self, bar: i64, read: usize, found: &[(String, Found, f64)]) -> anyhow::Result<()> {
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;
        tx.execute(
            "INSERT INTO signal_scan (bar, at, markets, found) VALUES ($1, $2, $3, $4)
             ON CONFLICT (bar) DO UPDATE SET at = EXCLUDED.at, markets = EXCLUDED.markets, found = EXCLUDED.found",
            &[&bar, &now_ms(), &(read as i32), &(found.len() as i32)],
        )
        .await?;
        for (market, f, price) in found {
            let drives = Value::Array(f.drives.iter().map(|d| json!({ "at": d.at, "price": d.price })).collect());
            let (turned_at, turn_price) = (f.turn.map(|t| t.at), f.turn.map(|t| t.price));
            tx.execute(
                "INSERT INTO signal (kind, market, side, timeframe, third_at, drives, turned_at, turn_price, score, divergence, price, found_bar, seen_bar)
                 VALUES ($1, $2, $3, '4h', $4, $5, $6, $7, $8, $9, $10, $11, $11)
                 ON CONFLICT (kind, market, side, timeframe, third_at) DO UPDATE SET
                     drives = EXCLUDED.drives, turned_at = COALESCE(signal.turned_at, EXCLUDED.turned_at),
                     turn_price = COALESCE(signal.turn_price, EXCLUDED.turn_price), score = EXCLUDED.score,
                     divergence = EXCLUDED.divergence, price = EXCLUDED.price, seen_bar = EXCLUDED.seen_bar",
                &[&KIND, market, &f.side.id(), &f.drives[2].at, &drives, &turned_at, &turn_price, &(f.score as i32), &f.divergence, price, &bar],
            )
            .await?;
        }
        tx.execute("DELETE FROM signal WHERE seen_bar < $1", &[&(bar - KEPT_MS)]).await?;
        tx.execute("DELETE FROM signal_scan WHERE bar < $1", &[&(bar - KEPT_MS)]).await?;
        tx.commit().await?;
        Ok(())
    }

    /// One message per reader who asked, naming the patterns that turned since the last.
    async fn tell(&self, bar: i64) -> anyhow::Result<()> {
        let client = self.pool.get().await?;
        let turned = client
            .query(
                "UPDATE signal SET told_at = $2 WHERE seen_bar = $1 AND turned_at IS NOT NULL AND told_at IS NULL
                 RETURNING market, side, drives, score, divergence, price",
                &[&bar, &now_ms()],
            )
            .await?;
        let Some(telegram) = self.telegram.as_ref().filter(|_| !turned.is_empty()) else { return Ok(()) };
        let mut lines: Vec<Told> = turned
            .iter()
            .map(|r| Told {
                market: r.get(0),
                bull: r.get::<_, String>(1) == "bull",
                drives: r.get::<_, Value>(2).as_array().map(|d| d.iter().filter_map(|p| p["price"].as_f64()).collect()).unwrap_or_default(),
                score: r.get(3),
                divergence: r.get(4),
                price: r.get(5),
            })
            .collect();
        lines.sort_by_key(|t| -t.score);
        for reader in client.query("SELECT owner FROM signal_watch WHERE notify", &[]).await? {
            let owner: String = reader.get(0);
            telegram.send_to_owner(&owner, |offset| signal_text(&lines, bar, offset)).await;
        }
        Ok(())
    }
}

/// A pattern that turned, as a message names it.
struct Told {
    market: String,
    bull: bool,
    drives: Vec<f64>,
    score: i32,
    divergence: bool,
    price: f64,
}

/// A price to five figures, as a chart of it would read: 13.600, 1,580.00.
fn price_text(price: f64) -> String {
    let digits = (4 - price.abs().log10().floor() as i32).clamp(2, 8);
    format_price(price, Some(digits as usize))
}

/// What the screener says when patterns turn, in the reader's own hours.
fn signal_text(turned: &[Told], bar: i64, offset: i32) -> String {
    let mut text = format!("🔔 Three drives · 4h — {} turned\n", turned.len());
    for t in turned.iter().take(TOLD_AT_MOST) {
        let name = MarketRef::parse(&t.market).map_or_else(|| t.market.clone(), |m| format!("{} ({})", m.symbol, m.venue.label()));
        let drives = t.drives.iter().map(|p| price_text(*p)).collect::<Vec<_>>().join(" → ");
        let (dot, way) = if t.bull { ("🟢", "bullish") } else { ("🔴", "bearish") };
        let divergence = if t.divergence { " · RSI divergence" } else { "" };
        text += &format!("\n{dot} {name} — {way}, score {}{divergence}\n   drives {drives} · now {}\n", t.score, price_text(t.price));
    }
    if turned.len() > TOLD_AT_MOST {
        text += &format!("\n…and {} more in the Signal panel\n", turned.len() - TOLD_AT_MOST);
    }
    text + &format!("\n{}", telegram::stamp(bar, offset))
}

// ---------------------------------------------------------------- the API

/// `GET /api/signals`: what the latest scan found, when it ran and when the
/// next will, and whether this reader is told on Telegram.
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let notify = client.query_opt("SELECT notify FROM signal_watch WHERE owner = $1", &[&owner]).await?.is_some_and(|r| r.get::<_, bool>(0));
    let linked = match state.telegram.as_ref() {
        Some(telegram) => !telegram.chats(&owner).await?.is_empty(),
        None => false,
    };
    let now = now_ms();
    let scan = client.query_opt("SELECT bar, at, markets FROM signal_scan ORDER BY bar DESC LIMIT 1", &[]).await?;
    let signals = match &scan {
        Some(scan) => client
            .query(
                "SELECT id, market, side, drives, turned_at, turn_price, score, divergence, price, found_bar FROM signal
                 WHERE seen_bar = $1 ORDER BY turned_at IS NULL, score DESC, market",
                &[&scan.get::<_, i64>(0)],
            )
            .await?
            .iter()
            .map(|r| {
                json!({
                    "id": r.get::<_, i64>(0),
                    "market": r.get::<_, String>(1),
                    "side": r.get::<_, String>(2),
                    "drives": r.get::<_, Value>(3),
                    "turnedAt": r.get::<_, Option<i64>>(4),
                    "turnPrice": r.get::<_, Option<f64>>(5),
                    "score": r.get::<_, i32>(6),
                    "divergence": r.get::<_, bool>(7),
                    "price": r.get::<_, f64>(8),
                    "foundAt": r.get::<_, i64>(9),
                })
            })
            .collect(),
        None => Vec::new(),
    };
    Ok(Json(json!({
        "scannedAt": scan.as_ref().map(|s| s.get::<_, i64>(1)),
        "bar": scan.as_ref().map(|s| s.get::<_, i64>(0)),
        "markets": scan.as_ref().map_or(0, |s| s.get::<_, i32>(2)),
        "nextAt": settled(now) + STEP + SETTLE_MS,
        "timeframe": "4h",
        "notify": notify,
        "telegram": state.telegram.is_some(),
        "linked": linked,
        "signals": signals,
    })))
}

/// `PUT /api/signals/notify` `{ "notify": true }`: whether this reader is told on Telegram.
pub async fn set_notify(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let notify = body["notify"].as_bool().ok_or_else(|| ApiError::bad_request("notify must be true or false"))?;
    state
        .pool
        .get()
        .await?
        .execute("INSERT INTO signal_watch (owner, notify) VALUES ($1, $2) ON CONFLICT (owner) DO UPDATE SET notify = EXCLUDED.notify", &[&owner, &notify])
        .await?;
    Ok(Json(json!({ "notify": notify })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scan_reads_up_to_the_bar_that_has_settled() {
        let close = 1_790_928_000_000; // a four-hour boundary
        assert_eq!(close % STEP, 0);
        assert_eq!(settled(close + 30_000), close - STEP, "half a minute after a close, that bar is still settling");
        assert_eq!(settled(close + SETTLE_MS), close);
        assert_eq!(settled(close + STEP - 1), close);
    }

    #[test]
    fn a_turn_reads_as_a_message() {
        let turned = [
            Told { market: "binance-perp:LINKUSDT".into(), bull: false, drives: vec![13.6, 14.4, 15.8], score: 86, divergence: true, price: 15.1 },
            Told { market: "bybit-perp:ZECUSDT".into(), bull: true, drives: vec![1580.0, 1520.0, 1490.0], score: 70, divergence: false, price: 1510.0 },
        ];
        let text = signal_text(&turned, 1_790_928_000_000, 7 * 60);
        assert!(text.starts_with("🔔 Three drives · 4h — 2 turned\n"), "{text}");
        assert!(text.contains("🔴 LINKUSDT (Binance Perp) — bearish, score 86 · RSI divergence\n   drives 13.600 → 14.400 → 15.800 · now 15.100"), "{text}");
        assert!(text.contains("🟢 ZECUSDT (Bybit Perp) — bullish, score 70\n   drives 1,580.00 → 1,520.00 → 1,490.00"), "{text}");
        assert!(text.ends_with("UTC+7"), "{text}");
    }
}
