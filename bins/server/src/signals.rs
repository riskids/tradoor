//! The Signal panel's screener. A minute after each four-hour bar closes, the
//! two hundred coins with the most turnover on Binance's and Bybit's USDT
//! perpetuals are read — five hundred closed bars each — and searched for three
//! drives either way (`drives`). A pattern is kept with the last scan that
//! still saw it: the panel lists what the latest scan saw, and the readers who
//! asked are told on Telegram, once, when a pattern turns.
//!
//! The record: a pattern that turned is entered at the close the scan read,
//! and each scan after weighs the bars since against its target and its stop
//! until one is reached or four days pass (`drives::outcome`). The same rules
//! are replayed weekly — and whenever they change — over some 166 days of
//! every coin's bars, bar by bar and seeing nothing past the bar a scan would
//! have read, for what the record would have been.

use std::collections::HashMap;
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
use crate::drives::{self, Found, Hit, Side};
use crate::telegram::{self, Telegram};

/// Coins screened, by a day's turnover.
const COINS: usize = 200;
/// Closed bars read per coin: enough for forty days of drives and their warm-up.
const BARS: u32 = 500;
/// Closed bars a replay reads per coin: as many as Bybit gives in one ask, some 166 days.
const HISTORY_BARS: u32 = 1000;
/// The rules' version. A change to `drives` bumps it, and the history is
/// replayed under the new rules after the next scan.
const VERSION: i32 = 2;
/// How old a replay may grow before it is run again.
const REPLAY_EVERY_MS: i64 = 7 * 24 * 3_600_000;
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
            if self.replay_due().await {
                if let Err(e) = self.replay(bar).await {
                    tracing::warn!(error = %e, "signal replay");
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
            futures_util::stream::iter(markets).map(|market| async move { (market.clone(), self.bars(&market, bar, BARS).await) }).buffer_unordered(AT_ONCE).collect().await;
        let mut held = HashMap::new();
        let mut found = Vec::new();
        for (market, bars) in results {
            match bars {
                Ok(bars) => {
                    let price = bars.last().map_or(0.0, |c| c.close);
                    for side in [Side::Bear, Side::Bull] {
                        if let Some(f) = drives::find(&bars, side) {
                            found.push((market.clone(), f, price));
                        }
                    }
                    held.insert(market, bars);
                }
                Err(e) => tracing::debug!(%market, error = %e, "signal scan: bars not read"),
            }
        }
        self.keep(bar, held.len(), &found).await?;
        tracing::info!(markets = held.len(), found = found.len(), secs = (now_ms() - started) / 1000, "signal scan");
        if let Err(e) = self.settle(bar, &held).await {
            tracing::warn!(error = %e, "weighing the patterns that turned");
        }
        self.tell(bar).await
    }

    /// Each pattern that turned and has not played out, against the bars since it was entered.
    async fn settle(&self, bar: i64, held: &HashMap<String, Vec<Candle>>) -> anyhow::Result<()> {
        let client = self.pool.get().await?;
        let open = client
            .query("SELECT id, market, side, entry_at, entry, target, stop FROM signal WHERE outcome IS NULL AND entry_at IS NOT NULL AND target IS NOT NULL", &[])
            .await?;
        for row in open {
            let (id, market, entry_at): (i64, String, i64) = (row.get(0), row.get(1), row.get(3));
            let side = if row.get::<_, String>(2) == "bull" { Side::Bull } else { Side::Bear };
            // A coin that has left the two hundred is still followed to its end.
            let fetched;
            let bars = match held.get(&market) {
                Some(bars) => bars,
                None => match self.bars(&market, bar, BARS).await {
                    Ok(bars) => {
                        fetched = bars;
                        &fetched
                    }
                    Err(_) => continue,
                },
            };
            let after: Vec<Candle> = bars.iter().filter(|k| k.open_time >= entry_at).cloned().collect();
            let o = drives::outcome(side, row.get(4), row.get(5), row.get(6), &after);
            let decided = o.result != Hit::Open;
            client
                .execute(
                    "UPDATE signal SET outcome = $2, outcome_at = $3, ret = $4, best = $5, worst = $6 WHERE id = $1",
                    &[&id, &decided.then(|| o.result.id()), &decided.then_some(bar), &o.ret, &o.best, &o.worst],
                )
                .await?;
        }
        Ok(())
    }

    async fn replay_due(&self) -> bool {
        let Ok(client) = self.pool.get().await else { return false };
        match client.query_opt("SELECT at, version FROM signal_backtest_run ORDER BY at DESC LIMIT 1", &[]).await {
            Ok(Some(run)) => run.get::<_, i32>(1) != VERSION || run.get::<_, i64>(0) < now_ms() - REPLAY_EVERY_MS,
            Ok(None) => true,
            Err(_) => false,
        }
    }

    /// The rules run over each coin's history as the scans would have run them,
    /// and how each pattern then played out.
    async fn replay(&self, bar: i64) -> anyhow::Result<()> {
        let started = now_ms();
        let markets = self.catalog.perp_leaders(COINS).await;
        if markets.is_empty() {
            bail!("the catalogue lists no perpetuals yet");
        }
        let results: Vec<(String, anyhow::Result<Vec<Candle>>)> =
            futures_util::stream::iter(markets).map(|market| async move { (market.clone(), self.bars(&market, bar, HISTORY_BARS).await) }).buffer_unordered(2).collect().await;
        let (mut read, mut from) = (0, bar);
        let mut rows = Replayed::default();
        for (market, bars) in results {
            let Ok(bars) = bars else { continue };
            read += 1;
            from = from.min(bars.first().map_or(bar, |c| c.open_time));
            for side in [Side::Bear, Side::Bull] {
                for (at, f) in drives::history(&bars, side) {
                    let entry = bars[at].close;
                    let o = drives::outcome(side, entry, f.target, f.stop, &bars[at + 1..]);
                    if matches!(o.result, Hit::Open | Hit::Passed) {
                        continue;
                    }
                    rows.push(&market, side, bars[at].open_time + STEP, entry, &f, &o);
                }
            }
        }
        let mut client = self.pool.get().await?;
        let tx = client.transaction().await?;
        let at = now_ms();
        tx.execute("INSERT INTO signal_backtest_run (at, version, markets, from_at, to_at) VALUES ($1, $2, $3, $4, $5)", &[&at, &VERSION, &(read as i32), &from, &bar]).await?;
        tx.execute(
            "INSERT INTO signal_backtest (run, market, side, entry_at, entry, target, stop, score, divergence, outcome, bars, ret, best, worst)
             SELECT $1, * FROM UNNEST($2::text[], $3::text[], $4::bigint[], $5::float8[], $6::float8[], $7::float8[], $8::int[], $9::bool[], $10::text[], $11::int[], $12::float8[], $13::float8[], $14::float8[])
             ON CONFLICT DO NOTHING",
            &[&at, &rows.market, &rows.side, &rows.entry_at, &rows.entry, &rows.target, &rows.stop, &rows.score, &rows.divergence, &rows.outcome, &rows.bars, &rows.ret, &rows.best, &rows.worst],
        )
        .await?;
        tx.execute("DELETE FROM signal_backtest_run WHERE at <> $1", &[&at]).await?;
        tx.commit().await?;
        tracing::info!(markets = read, patterns = rows.market.len(), secs = (now_ms() - started) / 1000, "signal replay");
        Ok(())
    }

    /// A market's last `count` closed four-hour bars up to `bar`, oldest first.
    async fn bars(&self, market: &str, bar: i64, count: u32) -> anyhow::Result<Vec<Candle>> {
        let m = MarketRef::parse(market).ok_or_else(|| anyhow!("{market} is not a market"))?;
        let exchange = m.venue.exchange();
        let url = match exchange {
            Exchange::Binance => binance::klines_url(binance::PERP_REST, &m, Interval::H4, None, None, count),
            Exchange::Bybit => bybit::klines_url(bybit::REST, &m, Interval::H4, None, None, count),
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
            // Entered where the first scan to see it turned read up to.
            let (entry_at, entry) = (f.turn.map(|_| bar), f.turn.map(|_| *price));
            tx.execute(
                "INSERT INTO signal (kind, market, side, timeframe, third_at, drives, turned_at, turn_price, score, divergence, price, found_bar, seen_bar, target, stop, entry_at, entry)
                 VALUES ($1, $2, $3, '4h', $4, $5, $6, $7, $8, $9, $10, $11, $11, $12, $13, $14, $15)
                 ON CONFLICT (kind, market, side, timeframe, third_at) DO UPDATE SET
                     drives = EXCLUDED.drives, turned_at = COALESCE(signal.turned_at, EXCLUDED.turned_at),
                     turn_price = COALESCE(signal.turn_price, EXCLUDED.turn_price), score = EXCLUDED.score,
                     divergence = EXCLUDED.divergence, price = EXCLUDED.price, seen_bar = EXCLUDED.seen_bar,
                     target = EXCLUDED.target, stop = EXCLUDED.stop,
                     entry_at = COALESCE(signal.entry_at, EXCLUDED.entry_at), entry = COALESCE(signal.entry, EXCLUDED.entry)",
                &[&KIND, market, &f.side.id(), &f.drives[2].at, &drives, &turned_at, &turn_price, &(f.score as i32), &f.divergence, price, &bar, &f.target, &f.stop, &entry_at, &entry],
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

/// Replayed patterns, a column each, for one insert.
#[derive(Default)]
struct Replayed {
    market: Vec<String>,
    side: Vec<&'static str>,
    entry_at: Vec<i64>,
    entry: Vec<f64>,
    target: Vec<f64>,
    stop: Vec<f64>,
    score: Vec<i32>,
    divergence: Vec<bool>,
    outcome: Vec<&'static str>,
    bars: Vec<i32>,
    ret: Vec<f64>,
    best: Vec<f64>,
    worst: Vec<f64>,
}

impl Replayed {
    fn push(&mut self, market: &str, side: Side, entry_at: i64, entry: f64, f: &Found, o: &drives::Outcome) {
        self.market.push(market.to_string());
        self.side.push(side.id());
        self.entry_at.push(entry_at);
        self.entry.push(entry);
        self.target.push(f.target);
        self.stop.push(f.stop);
        self.score.push(f.score as i32);
        self.divergence.push(f.divergence);
        self.outcome.push(o.result.id());
        self.bars.push(o.bars as i32);
        self.ret.push(o.ret);
        self.best.push(o.best);
        self.worst.push(o.worst);
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
    let row = |r: &tokio_postgres::Row| {
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
            "target": r.get::<_, Option<f64>>(10),
            "stop": r.get::<_, Option<f64>>(11),
            "entry": r.get::<_, Option<f64>>(12),
            "outcome": r.get::<_, Option<String>>(13),
            "ret": r.get::<_, Option<f64>>(14),
            "lastSeen": r.get::<_, i64>(15),
        })
    };
    const COLUMNS: &str = "id, market, side, drives, turned_at, turn_price, score, divergence, price, found_bar, target, stop, entry, outcome, ret, seen_bar";
    let (signals, ended) = match &scan {
        Some(scan) => {
            let bar: i64 = scan.get(0);
            let now = client.query(&format!("SELECT {COLUMNS} FROM signal WHERE seen_bar = $1 ORDER BY turned_at IS NULL, score DESC, market"), &[&bar]).await?;
            // Turned, and gone from the list within the last day: the reader saw them, and should see why they went.
            let gone = client
                .query(&format!("SELECT {COLUMNS} FROM signal WHERE turned_at IS NOT NULL AND seen_bar < $1 AND seen_bar >= $1 - $2 ORDER BY seen_bar DESC, score DESC"), &[&bar, &(6 * STEP)])
                .await?;
            (now.iter().map(row).collect::<Vec<_>>(), gone.iter().map(row).collect::<Vec<_>>())
        }
        None => (Vec::new(), Vec::new()),
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
        "ended": ended,
    })))
}

/// One pattern's result, as the record counts it.
struct Played {
    side: String,
    score: i32,
    divergence: bool,
    outcome: String,
    ret: f64,
    best: f64,
    worst: f64,
}

/// The record by side, and within a side by score and by divergence: how many,
/// how many reached the target first, the stop first or neither in four days,
/// and the average move — to the exit, and at best and worst on the way.
fn summarize(played: &[Played]) -> Vec<Value> {
    let groups: [(&str, fn(&Played) -> bool); 5] = [
        ("all", |_| true),
        ("70", |p| p.score >= 70),
        ("50", |p| (50..70).contains(&p.score)),
        ("0", |p| p.score < 50),
        ("div", |p| p.divergence),
    ];
    let mut out = Vec::new();
    for side in ["bear", "bull"] {
        for (group, keep) in groups {
            let rows: Vec<&Played> = played.iter().filter(|p| p.side == side && p.outcome != "passed" && keep(p)).collect();
            let n = rows.len();
            let count = |o: &str| rows.iter().filter(|p| p.outcome == o).count();
            let mean = |f: fn(&Played) -> f64| if n == 0 { 0.0 } else { rows.iter().map(|p| f(p)).sum::<f64>() / n as f64 };
            out.push(json!({
                "side": side,
                "group": group,
                "n": n,
                "target": count("target"),
                "stop": count("stop"),
                "expired": count("expired"),
                "ret": mean(|p| p.ret),
                "best": mean(|p| p.best),
                "worst": mean(|p| p.worst),
            }));
        }
    }
    out
}

/// `GET /api/signals/stats`: the record — the latest replay of the history,
/// the live patterns since the screener began, and the latest of those.
pub async fn stats(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    owner(&headers)?;
    let client = state.pool.get().await?;
    let played = |r: &tokio_postgres::Row| Played { side: r.get(0), score: r.get(1), divergence: r.get(2), outcome: r.get(3), ret: r.get(4), best: r.get(5), worst: r.get(6) };
    let backtest = match client.query_opt("SELECT at, version, markets, from_at, to_at FROM signal_backtest_run ORDER BY at DESC LIMIT 1", &[]).await? {
        Some(run) => {
            let at: i64 = run.get(0);
            let rows = client.query("SELECT side, score, divergence, outcome, ret, best, worst FROM signal_backtest WHERE run = $1", &[&at]).await?;
            json!({
                "at": at,
                "version": run.get::<_, i32>(1),
                "markets": run.get::<_, i32>(2),
                "from": run.get::<_, i64>(3),
                "to": run.get::<_, i64>(4),
                "groups": summarize(&rows.iter().map(played).collect::<Vec<_>>()),
            })
        }
        None => Value::Null,
    };
    let decided = client.query("SELECT side, score, divergence, outcome, ret, best, worst FROM signal WHERE outcome IS NOT NULL", &[]).await?;
    let since = client.query_one("SELECT min(entry_at), count(*) FILTER (WHERE outcome IS NULL) FROM signal WHERE entry_at IS NOT NULL", &[]).await?;
    let recent = client
        .query(
            "SELECT id, market, side, drives, score, divergence, entry_at, entry, target, stop, outcome, outcome_at, ret, best, worst FROM signal
             WHERE entry_at IS NOT NULL ORDER BY entry_at DESC, score DESC LIMIT 60",
            &[],
        )
        .await?
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<_, i64>(0),
                "market": r.get::<_, String>(1),
                "side": r.get::<_, String>(2),
                "drives": r.get::<_, Value>(3),
                "score": r.get::<_, i32>(4),
                "divergence": r.get::<_, bool>(5),
                "entryAt": r.get::<_, i64>(6),
                "entry": r.get::<_, Option<f64>>(7),
                "target": r.get::<_, Option<f64>>(8),
                "stop": r.get::<_, Option<f64>>(9),
                "outcome": r.get::<_, Option<String>>(10),
                "outcomeAt": r.get::<_, Option<i64>>(11),
                "ret": r.get::<_, Option<f64>>(12),
                "best": r.get::<_, Option<f64>>(13),
                "worst": r.get::<_, Option<f64>>(14),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "version": VERSION,
        "windowBars": drives::OUTCOME_BARS,
        "backtest": backtest,
        "live": {
            "since": since.get::<_, Option<i64>>(0),
            "open": since.get::<_, i64>(1),
            "groups": summarize(&decided.iter().map(played).collect::<Vec<_>>()),
        },
        "recent": recent,
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
    fn the_record_counts_each_side_by_score_and_divergence() {
        let p = |side: &str, score: i32, divergence: bool, outcome: &str, ret: f64| Played { side: side.into(), score, divergence, outcome: outcome.into(), ret, best: ret.max(0.0), worst: ret.min(0.0) };
        let played = [p("bear", 80, true, "target", 6.0), p("bear", 75, false, "stop", -2.0), p("bear", 40, true, "expired", 1.0), p("bear", 60, false, "passed", 0.0), p("bull", 55, false, "target", 4.0)];
        let groups = summarize(&played);
        let find = |side: &str, group: &str| groups.iter().find(|g| g["side"] == side && g["group"] == group).unwrap().clone();
        let all = find("bear", "all");
        assert_eq!((all["n"].as_u64(), all["target"].as_u64(), all["stop"].as_u64(), all["expired"].as_u64()), (Some(3), Some(1), Some(1), Some(1)), "a pattern already past its target is no trade");
        assert!((all["ret"].as_f64().unwrap() - 5.0 / 3.0).abs() < 1e-9);
        assert_eq!(find("bear", "70")["n"].as_u64(), Some(2));
        assert_eq!(find("bear", "div")["target"].as_u64(), Some(1));
        assert_eq!(find("bull", "50")["n"].as_u64(), Some(1));
        assert_eq!(find("bull", "70")["n"].as_u64(), Some(0));
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
