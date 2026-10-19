//! The trading journal. Every long or short position drawn on a chart is a
//! trade here — the drawing's entry, stop and target, and the coins its risk
//! settings buy — kept in step as the drawing moves (`sync_drawing`, called by
//! the drawings API), and left in the journal, untied, when the drawing goes.
//! A trade can be added by hand too.
//!
//! Price decides how each went. A watcher reads each market's closed minutes
//! from where it left off and walks every trade still planned or open through
//! them (`Trade::walk`): open once price reaches the entry, from the moment
//! the position was drawn — or from its left edge, for a box drawn wholly on
//! bars already past, a setup looked back on — then a win at the target or a
//! loss at the stop; a minute that reaches both counts as the stop. A status set by hand — closed at a price,
//! break-even, cancelled — takes the trade out of price's hands until it is
//! given back, and moving the drawing weighs it again from the start.
//!
//! The minutes come from the market's own exchange: Binance, Bybit,
//! Hyperliquid, OKX and Bitget. A trade on another venue, or on an index, is
//! kept by hand.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hasher};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};
use tradoor_core::{Candle, ChartTimeframe, Interval};
use tradoor_store::now_ms;
use tradoor_venues::{binance, bitget, bybit, hyperliquid, okx, Exchange, Kind, MarketRef};

use crate::api::{owner, ApiError, ApiResult, AppState};
use crate::catalog::Catalog;
use crate::dns::Doh;
use crate::drawings::Point;

const MINUTE: i64 = 60_000;
/// How often the watcher reads the markets with trades still in play.
const WATCH_EVERY: Duration = Duration::from_secs(60);
/// Trades walked per round, the longest unread first.
const TRADES_PER_ROUND: i64 = 500;
/// Pages of minutes one market reads per round: a position drawn on old bars
/// catches up over a few rounds rather than holding up the rest.
const PAGES_PER_ROUND: usize = 10;
/// Trades one browser keeps.
const MAX_PER_OWNER: i64 = 50_000;
const MAX_TEXT: usize = 4_000;

/// The figures a position's settings fall back on, as the chart draws them
/// (crates/render, `paint_position`): a 1,000 account risking 1%, or 10 in cash.
const DEFAULT_ACCOUNT: f64 = 1_000.0;
const DEFAULT_RISK_PCT: f64 = 1.0;
const DEFAULT_RISK_CASH: f64 = 10.0;

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
}

const STATUSES: [&str; 7] = ["planned", "open", "win", "loss", "closed", "breakeven", "cancelled"];

/// A position as drawn: from when price is weighed against it, its prices,
/// and what it risks and buys.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub side: Side,
    pub start_at: i64,
    pub entry: f64,
    pub stop: f64,
    pub target: f64,
    pub risk: f64,
    pub qty: f64,
}

/// A style value the page may have written as a number or as a string.
fn style_num(style: &Map<String, Value>, key: &str) -> Option<f64> {
    let v = style.get(key)?;
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok()).filter(|v: &f64| v.is_finite() && *v > 0.0)
}

/// What a position risks, as its settings say: cash, or a part of the account.
pub fn risk_of(style: &Map<String, Value>) -> f64 {
    let cash = style.get("riskUnit").and_then(Value::as_str).is_some_and(|u| u != "%");
    if cash {
        style_num(style, "risk").unwrap_or(DEFAULT_RISK_CASH)
    } else {
        style_num(style, "account").unwrap_or(DEFAULT_ACCOUNT) * style_num(style, "risk").unwrap_or(DEFAULT_RISK_PCT) / 100.0
    }
}

/// A long or short drawing's plan: the entry, stop and target its three
/// anchors hold. A box that reaches the bar forming when it was drawn, or
/// past it, is a trade taken then, weighed from that moment — however far
/// back its left edge sits; one wholly on bars already past is a setup looked
/// back on, weighed from its left edge. None for anything else, or prices
/// that make no trade.
pub fn plan_of(kind: &str, points: &[Point], style: &Map<String, Value>, drawn_at: i64, timeframe: &str) -> Option<Plan> {
    let side = Side::of(kind)?;
    let [entry, stop, target] = [points.first()?.price, points.get(1)?.price, points.get(2)?.price];
    let sound = match side {
        Side::Long => stop < entry && entry < target,
        Side::Short => target < entry && entry < stop,
    };
    if !sound || entry <= 0.0 {
        return None;
    }
    let left = points.iter().map(|p| p.time).fold(f64::INFINITY, f64::min) as i64;
    let right = points.iter().map(|p| p.time).fold(f64::NEG_INFINITY, f64::max) as i64;
    let bar = ChartTimeframe::parse(timeframe).and_then(|tf| Some(tf.source().interval.ms()? * i64::from(tf.source().group))).unwrap_or(31 * 24 * 60 * MINUTE);
    let start_at = if right >= drawn_at - bar { drawn_at } else { left };
    let risk = risk_of(style);
    Some(Plan { side, start_at: start_at.div_euclid(MINUTE) * MINUTE, entry, stop, target, risk, qty: risk / (entry - stop).abs() })
}

/// How a trade stands against price.
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub side: Side,
    pub entry: f64,
    pub stop: f64,
    pub target: f64,
    pub status: String,
    pub opened_at: Option<i64>,
    pub closed_at: Option<i64>,
    pub exit: Option<f64>,
    /// The open of the next minute to read.
    pub checked_until: i64,
}

impl Trade {
    /// Walk closed minutes, oldest first, from `checked_until` on. The minute
    /// the entry is reached may reach the stop or the target too: the stop
    /// first, as the cautious reading, then the target.
    pub fn walk(&mut self, bars: &[Candle]) {
        let long = self.side == Side::Long;
        let mut prev_close: Option<f64> = None;
        for k in bars {
            if k.open_time < self.checked_until {
                prev_close = Some(k.close);
                continue;
            }
            if self.status == "planned" {
                // Reached within the minute, or jumped over between two.
                let within = k.low <= self.entry && self.entry <= k.high;
                let gapped = prev_close.is_some_and(|c| (c - self.entry).signum() != (k.open - self.entry).signum());
                if within || gapped {
                    self.status = "open".into();
                    self.opened_at = Some(k.open_time);
                }
            }
            if self.status == "open" {
                let stopped = if long { k.low <= self.stop } else { k.high >= self.stop };
                let taken = if long { k.high >= self.target } else { k.low <= self.target };
                if stopped || taken {
                    (self.status, self.exit) = if stopped { ("loss".into(), Some(self.stop)) } else { ("win".into(), Some(self.target)) };
                    self.closed_at = Some(k.open_time);
                }
            }
            self.checked_until = k.open_time + MINUTE;
            prev_close = Some(k.close);
            if self.status != "planned" && self.status != "open" {
                return;
            }
        }
    }
}

/// Whether price can decide a trade on this market: its exchange's minutes are read here.
pub fn tracked(market: &str) -> bool {
    MarketRef::parse(market).is_some_and(|m| matches!(m.venue.exchange(), Exchange::Binance | Exchange::Bybit | Exchange::Hyperliquid | Exchange::Okx | Exchange::Bitget))
}

// ---------------------------------------------------------------- the drawings' side

/// Keep the trade of a long or short drawing in step with it: made when the
/// drawing is, its plan moved when the drawing moves — and then weighed
/// against price again from the start. A drawing put back by an undo takes
/// back the trade it had rather than starting another. Anything else, and a
/// ratio chart's positions, are no trade.
pub async fn sync_drawing(client: &tokio_postgres::Client, owner: &str, drawing: &Value) -> Result<(), tokio_postgres::Error> {
    let (Some(id), Some(market), Some(kind)) = (drawing["id"].as_i64(), drawing["market"].as_str(), drawing["kind"].as_str()) else { return Ok(()) };
    if Side::of(kind).is_none() || MarketRef::parse(market).is_none() {
        return Ok(());
    }
    let points: Vec<Point> = drawing["points"].as_array().map_or_else(Vec::new, |a| {
        a.iter().filter_map(|p| Some(Point { time: p["time"].as_f64()?, price: p["price"].as_f64()? })).collect()
    });
    let style = drawing["style"].as_object().cloned().unwrap_or_default();
    let timeframe = drawing["timeframe"].as_str().unwrap_or("");
    let drawn_at = drawing["createdAt"].as_i64().unwrap_or_else(now_ms);
    let Some(plan) = plan_of(kind, &points, &style, drawn_at, timeframe) else { return Ok(()) };
    let now = now_ms();
    let held = client
        .query_opt("SELECT id, start_at, entry, stop, target, auto FROM journal_trade WHERE drawing_id = $1 AND owner = $2", &[&id, &owner])
        .await?;
    if let Some(row) = held {
        let moved = row.get::<_, i64>(1) != plan.start_at || row.get::<_, f64>(2) != plan.entry || row.get::<_, f64>(3) != plan.stop || row.get::<_, f64>(4) != plan.target;
        // A moved plan, still price's to decide, is weighed again from its start.
        let again = moved && row.get::<_, bool>(5);
        client
            .execute(
                "UPDATE journal_trade SET start_at = $2, entry = $3, stop = $4, target = $5, risk = $6, qty = $7, timeframe = $8, updated_at = $9,
                     status = CASE WHEN $10 THEN 'planned' ELSE status END, opened_at = CASE WHEN $10 THEN NULL ELSE opened_at END,
                     closed_at = CASE WHEN $10 THEN NULL ELSE closed_at END, exit = CASE WHEN $10 THEN NULL ELSE exit END,
                     checked_until = CASE WHEN $10 THEN $2 ELSE checked_until END
                 WHERE id = $1",
                &[&row.get::<_, i64>(0), &plan.start_at, &plan.entry, &plan.stop, &plan.target, &plan.risk, &plan.qty, &timeframe, &now, &again],
            )
            .await?;
        return Ok(());
    }
    // Undone deletes come back as new drawings, made just now: the trade they
    // left behind — the same prices on the same market — is theirs again.
    let back = client
        .execute(
            "UPDATE journal_trade SET drawing_id = $1, updated_at = $8 WHERE id = (
                 SELECT id FROM journal_trade WHERE owner = $2 AND drawing_id IS NULL AND market = $3 AND side = $4
                     AND entry = $5 AND stop = $6 AND target = $7 ORDER BY id DESC LIMIT 1)",
            &[&id, &owner, &market, &plan.side.id(), &plan.entry, &plan.stop, &plan.target, &now],
        )
        .await?;
    if back > 0 {
        return Ok(());
    }
    client
        .execute(
            "INSERT INTO journal_trade (owner, drawing_id, market, side, timeframe, start_at, entry, stop, target, risk, qty, checked_until, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $6, $12, $12) ON CONFLICT (drawing_id) DO NOTHING",
            &[&owner, &id, &market, &plan.side.id(), &timeframe, &plan.start_at, &plan.entry, &plan.stop, &plan.target, &plan.risk, &plan.qty, &now],
        )
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- the watcher

pub fn start(pool: deadpool_postgres::Pool, doh: &Arc<Doh>, catalog: Arc<Catalog>) {
    let http = reqwest::Client::builder().dns_resolver(doh.resolver()).timeout(Duration::from_secs(20)).build().unwrap_or_default();
    tokio::spawn(async move {
        loop {
            if let Err(e) = round(&pool, &http, &catalog).await {
                tracing::warn!(error = %e, "weighing the journal's trades");
            }
            tokio::time::sleep(WATCH_EVERY).await;
        }
    });
}

/// One round: each market with a trade in play read from its least-read
/// trade on, and every such trade walked through what was read.
async fn round(pool: &deadpool_postgres::Pool, http: &reqwest::Client, catalog: &Catalog) -> anyhow::Result<()> {
    let client = pool.get().await?;
    let rows = client
        .query(
            "SELECT id, market, side, entry, stop, target, status, opened_at, checked_until FROM journal_trade
             WHERE auto AND status IN ('planned', 'open') ORDER BY checked_until LIMIT $1",
            &[&TRADES_PER_ROUND],
        )
        .await?;
    let mut by_market: HashMap<String, Vec<(i64, Trade)>> = HashMap::new();
    for r in &rows {
        let market: String = r.get(1);
        if !tracked(&market) {
            continue;
        }
        let Some(side) = Side::of(r.get(2)) else { continue };
        let trade = Trade { side, entry: r.get(3), stop: r.get(4), target: r.get(5), status: r.get(6), opened_at: r.get(7), closed_at: None, exit: None, checked_until: r.get(8) };
        by_market.entry(market).or_default().push((r.get(0), trade));
    }
    let settled = now_ms().div_euclid(MINUTE) * MINUTE;
    for (market, mut trades) in by_market {
        let from = trades.iter().map(|(_, t)| t.checked_until).min().unwrap_or(settled);
        let bars = match minutes(http, catalog, &market, from, settled).await {
            Ok(bars) => bars,
            Err(e) => {
                tracing::debug!(%market, error = %e, "journal: minutes not read");
                continue;
            }
        };
        for (id, trade) in &mut trades {
            let before = (trade.status.clone(), trade.checked_until);
            trade.walk(&bars);
            if (trade.status.clone(), trade.checked_until) == before {
                continue;
            }
            client
                .execute(
                    "UPDATE journal_trade SET status = $2, opened_at = $3, closed_at = $4, exit = $5, checked_until = $6 WHERE id = $1 AND auto",
                    &[id, &trade.status, &trade.opened_at, &trade.closed_at, &trade.exit, &trade.checked_until],
                )
                .await?;
        }
    }
    Ok(())
}

/// A market's closed minutes from `from` up to `until`, oldest first, a few
/// pages' worth at most: the rest are read next round.
async fn minutes(http: &reqwest::Client, catalog: &Catalog, market: &str, from: i64, until: i64) -> anyhow::Result<Vec<Candle>> {
    let m = MarketRef::parse(market).ok_or_else(|| anyhow!("{market} is not a market"))?;
    let native = catalog.native_names(m.venue, std::slice::from_ref(&m.symbol)).await.remove(&m.symbol).unwrap_or_else(|| m.symbol.clone());
    let mut out: Vec<Candle> = Vec::new();
    let mut at = from;
    for _ in 0..PAGES_PER_ROUND {
        if at >= until {
            break;
        }
        let page = minute_page(http, &m, &native, at, until).await?;
        let page: Vec<Candle> = page.into_iter().filter(|k| k.open_time >= at && k.open_time + MINUTE <= until).collect();
        let Some(last) = page.last().map(|k| k.open_time) else { break };
        out.extend(page);
        at = last + MINUTE;
    }
    Ok(out)
}

fn candle(t: i64, o: f64, h: f64, l: f64, c: f64) -> Candle {
    Candle { open_time: t, open: o, high: h, low: l, close: c, volume: 0.0, quote_volume: None }
}

/// A number the exchanges send as a string or as a number.
fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

async fn text(http: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    let resp = http.get(url).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        bail!("{status} from {url}");
    }
    Ok(body)
}

/// One page of minutes from `from` on, as the market's exchange gives them.
async fn minute_page(http: &reqwest::Client, m: &MarketRef, native: &str, from: i64, until: i64) -> anyhow::Result<Vec<Candle>> {
    let rows_of = |body: &str, path: &[&str]| -> anyhow::Result<Vec<Value>> {
        let mut v: Value = serde_json::from_str(body)?;
        for key in path {
            v = v.get(*key).cloned().unwrap_or(Value::Null);
        }
        Ok(v.as_array().cloned().unwrap_or_default())
    };
    let arrays = |rows: Vec<Value>| -> Vec<Candle> {
        let mut out: Vec<Candle> = rows
            .iter()
            .filter_map(|r| Some(candle(num(r.get(0)?)? as i64, num(r.get(1)?)?, num(r.get(2)?)?, num(r.get(3)?)?, num(r.get(4)?)?)))
            .collect();
        out.sort_by_key(|k| k.open_time);
        out
    };
    let klines = |list: Vec<tradoor_venues::OwnedKline>| -> Vec<Candle> {
        list.iter()
            .filter_map(|k| Some(candle(k.open_time, k.open.parse().ok()?, k.high.parse().ok()?, k.low.parse().ok()?, k.close.parse().ok()?)))
            .collect()
    };
    Ok(match m.venue.exchange() {
        Exchange::Binance => {
            let base = if m.venue.kind() == Kind::Perp { binance::PERP_REST } else { binance::SPOT_REST };
            klines(binance::parse_klines(&text(http, &binance::klines_url(base, m, Interval::M1, Some(from), None, 1000)).await?)?)
        }
        Exchange::Bybit => {
            // Bybit answers the newest of a range: a range of one page.
            let end = (from + 1000 * MINUTE - 1).min(until);
            klines(bybit::parse_klines(&text(http, &bybit::klines_url(bybit::REST, m, Interval::M1, Some(from), Some(end), 1000)).await?)?)
        }
        Exchange::Hyperliquid => {
            let body = json!({ "type": "candleSnapshot", "req": { "coin": native, "interval": "1m", "startTime": from, "endTime": (from + 5000 * MINUTE).min(until) } });
            let resp = http.post(hyperliquid::INFO).header("content-type", "application/json").body(body.to_string()).send().await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                bail!("{status} from Hyperliquid");
            }
            let mut out: Vec<Candle> = rows_of(&text, &[])?
                .iter()
                .filter_map(|k| Some(candle(k["t"].as_i64()?, num(&k["o"])?, num(&k["h"])?, num(&k["l"])?, num(&k["c"])?)))
                .collect();
            out.sort_by_key(|k| k.open_time);
            out
        }
        Exchange::Okx => {
            // Newer than `before`, older than `after`, a hundred at most.
            let url = format!("{}/api/v5/market/history-candles?instId={native}&bar=1m&before={}&after={}&limit=100", okx::REST, from - 1, from + 100 * MINUTE);
            arrays(rows_of(&text(http, &url).await?, &["data"])?)
        }
        Exchange::Bitget => {
            let end = (from + 200 * MINUTE).min(until);
            let url = if m.venue.kind() == Kind::Perp {
                format!("{}/api/v2/mix/market/history-candles?symbol={native}&productType=usdt-futures&granularity=1m&endTime={end}&limit=200", bitget::REST)
            } else {
                format!("{}/api/v2/spot/market/history-candles?symbol={native}&granularity=1min&endTime={end}&limit=200", bitget::REST)
            };
            arrays(rows_of(&text(http, &url).await?, &["data"])?)
        }
        _ => bail!("{} has no minutes read here", m.venue.id()),
    })
}

// ---------------------------------------------------------------- the API

const COLUMNS: &str = "id, drawing_id, market, side, timeframe, start_at, entry, stop, target, risk, qty, status, opened_at, closed_at, exit, fees, auto, strategy, notes, created_at, updated_at";

fn row_json(r: &tokio_postgres::Row) -> Value {
    let market: String = r.get(2);
    json!({
        "id": r.get::<_, i64>(0),
        "drawingId": r.get::<_, Option<i64>>(1),
        "tracked": tracked(&market),
        "market": market,
        "side": r.get::<_, String>(3),
        "timeframe": r.get::<_, String>(4),
        "startAt": r.get::<_, i64>(5),
        "entry": r.get::<_, f64>(6),
        "stop": r.get::<_, f64>(7),
        "target": r.get::<_, f64>(8),
        "risk": r.get::<_, f64>(9),
        "qty": r.get::<_, f64>(10),
        "status": r.get::<_, String>(11),
        "openedAt": r.get::<_, Option<i64>>(12),
        "closedAt": r.get::<_, Option<i64>>(13),
        "exit": r.get::<_, Option<f64>>(14),
        "fees": r.get::<_, f64>(15),
        "auto": r.get::<_, bool>(16),
        "strategy": r.get::<_, String>(17),
        "notes": r.get::<_, String>(18),
        "createdAt": r.get::<_, i64>(19),
        "updatedAt": r.get::<_, i64>(20),
    })
}

/// `GET /api/journal`: this browser's trades, newest first, tagged with what
/// they read as: an open journal asks again every half minute, and while
/// nothing has changed the tag it sends back is answered 304, with no body.
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let rows = client.query(&format!("SELECT {COLUMNS} FROM journal_trade WHERE owner = $1 ORDER BY start_at DESC, id DESC"), &[&owner]).await?;
    let body = json!({ "trades": rows.iter().map(row_json).collect::<Vec<_>>() }).to_string();
    let tag = tag_of(&body);
    // A proxy that recompresses the answer weakens the tag: `W/"…"` still matches.
    let held = headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).is_some_and(|v| v.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    if held {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag)]).into_response());
    }
    Ok(([(header::CONTENT_TYPE, "application/json".to_owned()), (header::CACHE_CONTROL, "no-store".to_owned()), (header::ETAG, tag)], body).into_response())
}

fn tag_of(body: &str) -> String {
    let mut h = DefaultHasher::new();
    h.write(body.as_bytes());
    format!("\"{:016x}\"", h.finish())
}

fn finite(b: &Map<String, Value>, key: &str) -> ApiResult<Option<f64>> {
    match b.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|v| v.is_finite()).map(Some).ok_or_else(|| ApiError::bad_request(format!("{key} must be a finite number"))),
    }
}

fn short_text(b: &Map<String, Value>, key: &str) -> ApiResult<Option<String>> {
    match b.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(String::new())),
        Some(v) => {
            let s = v.as_str().ok_or_else(|| ApiError::bad_request(format!("{key} must be a string")))?;
            if s.len() > MAX_TEXT {
                return Err(ApiError::bad_request(format!("{key} may be at most {MAX_TEXT} bytes")));
            }
            Ok(Some(s.trim().to_string()))
        }
    }
}

/// `POST /api/journal` `{market, side, entry, stop, target, risk?, startAt?,
/// status?, exit?, closedAt?, strategy?, notes?, fees?}`: a trade added by
/// hand. One already closed is the hand's to keep; one planned or open is
/// price's to follow, where its market's minutes are read.
pub async fn create(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner = owner(&headers)?;
    let b = body.as_object().ok_or_else(|| ApiError::bad_request("body must be a JSON object"))?;
    let market = b.get("market").and_then(Value::as_str).and_then(MarketRef::parse).ok_or_else(|| ApiError::bad_request("market must be venue:SYMBOL"))?.to_string();
    let side = b.get("side").and_then(Value::as_str).and_then(Side::of).ok_or_else(|| ApiError::bad_request("side must be long or short"))?;
    let need = |key: &str| finite(b, key)?.filter(|v| *v > 0.0).ok_or_else(|| ApiError::bad_request(format!("{key} must be a positive number")));
    let (entry, stop, target) = (need("entry")?, need("stop")?, need("target")?);
    let sound = match side {
        Side::Long => stop < entry && entry < target,
        Side::Short => target < entry && entry < stop,
    };
    if !sound {
        return Err(ApiError::bad_request("a long's stop is under its entry and its target over; a short's the other way"));
    }
    let now = now_ms();
    let risk = finite(b, "risk")?.filter(|v| *v > 0.0).unwrap_or(DEFAULT_ACCOUNT * DEFAULT_RISK_PCT / 100.0);
    let start_at = finite(b, "startAt")?.map_or(now, |v| v as i64).div_euclid(MINUTE) * MINUTE;
    let status = b.get("status").and_then(Value::as_str).unwrap_or("planned");
    if !STATUSES.contains(&status) {
        return Err(ApiError::bad_request(format!("status is one of {}", STATUSES.join(", "))));
    }
    let exit = finite(b, "exit")?;
    let done = !matches!(status, "planned" | "open");
    if matches!(status, "win" | "loss" | "closed") && exit.is_none() {
        return Err(ApiError::bad_request("a closed trade needs its exit price"));
    }
    let closed_at = finite(b, "closedAt")?.map(|v| v as i64).or(done.then_some(now));
    let client = state.pool.get().await?;
    let held: i64 = client.query_one("SELECT count(*) FROM journal_trade WHERE owner = $1", &[&owner]).await?.get(0);
    if held >= MAX_PER_OWNER {
        return Err(ApiError::bad_request(format!("a browser may keep at most {MAX_PER_OWNER} trades")));
    }
    let row = client
        .query_one(
            &format!(
                "INSERT INTO journal_trade (owner, market, side, start_at, entry, stop, target, risk, qty, status, opened_at, closed_at, exit, fees, auto, checked_until, strategy, notes, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $4, $16, $17, $18, $18) RETURNING {COLUMNS}"
            ),
            &[
                &owner,
                &market,
                &side.id(),
                &start_at,
                &entry,
                &stop,
                &target,
                &risk,
                &(risk / (entry - stop).abs()),
                &status,
                &(status != "planned").then_some(start_at),
                &closed_at.filter(|_| done),
                &exit.filter(|_| done),
                &finite(b, "fees")?.unwrap_or(0.0),
                &!done,
                &short_text(b, "strategy")?.unwrap_or_default(),
                &short_text(b, "notes")?.unwrap_or_default(),
                &now,
            ],
        )
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "trade": row_json(&row) }))))
}

/// `PATCH /api/journal/{id}`: its strategy, notes, fees or risk; or its
/// status by hand — `closed` at `exit` (and `closedAt`), `breakeven`,
/// `cancelled` — which takes it out of price's hands; or `auto: true`, which
/// gives it back, weighed again from its start.
pub async fn update(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let b = body.as_object().ok_or_else(|| ApiError::bad_request("body must be a JSON object"))?;
    let client = state.pool.get().await?;
    let row = client
        .query_opt("SELECT entry, stop, start_at FROM journal_trade WHERE id = $1 AND owner = $2", &[&id, &owner])
        .await?
        .ok_or_else(|| ApiError::not_found(format!("trade {id} not found")))?;
    let (entry, stop, start_at): (f64, f64, i64) = (row.get(0), row.get(1), row.get(2));
    let now = now_ms();
    if let Some(strategy) = short_text(b, "strategy")? {
        client.execute("UPDATE journal_trade SET strategy = $2 WHERE id = $1", &[&id, &strategy]).await?;
    }
    if let Some(notes) = short_text(b, "notes")? {
        client.execute("UPDATE journal_trade SET notes = $2 WHERE id = $1", &[&id, &notes]).await?;
    }
    if let Some(fees) = finite(b, "fees")? {
        client.execute("UPDATE journal_trade SET fees = $2 WHERE id = $1", &[&id, &fees]).await?;
    }
    if let Some(risk) = finite(b, "risk")? {
        if risk <= 0.0 {
            return Err(ApiError::bad_request("risk must be positive"));
        }
        client.execute("UPDATE journal_trade SET risk = $2, qty = $2 / $3 WHERE id = $1", &[&id, &risk, &(entry - stop).abs()]).await?;
    }
    if b.get("auto").and_then(Value::as_bool) == Some(true) {
        client
            .execute(
                "UPDATE journal_trade SET auto = true, status = 'planned', opened_at = NULL, closed_at = NULL, exit = NULL, checked_until = start_at WHERE id = $1",
                &[&id],
            )
            .await?;
    }
    if let Some(status) = b.get("status").and_then(Value::as_str) {
        let closed_at = finite(b, "closedAt")?.map_or(now, |v| v as i64);
        match status {
            "closed" => {
                let exit = finite(b, "exit")?.filter(|v| *v > 0.0).ok_or_else(|| ApiError::bad_request("closing by hand needs the exit price"))?;
                client
                    .execute(
                        "UPDATE journal_trade SET auto = false, status = 'closed', exit = $2, closed_at = $3, opened_at = COALESCE(opened_at, $4) WHERE id = $1",
                        &[&id, &exit, &closed_at, &start_at],
                    )
                    .await?;
            }
            "breakeven" => {
                client
                    .execute(
                        "UPDATE journal_trade SET auto = false, status = 'breakeven', exit = entry, closed_at = $2, opened_at = COALESCE(opened_at, $3) WHERE id = $1",
                        &[&id, &closed_at, &start_at],
                    )
                    .await?;
            }
            "cancelled" => {
                client.execute("UPDATE journal_trade SET auto = false, status = 'cancelled', exit = NULL, closed_at = $2 WHERE id = $1", &[&id, &closed_at]).await?;
            }
            other => return Err(ApiError::bad_request(format!("a status set by hand is closed, breakeven or cancelled, not '{other}'"))),
        }
    }
    let row = client
        .query_one(&format!("UPDATE journal_trade SET updated_at = $2 WHERE id = $1 RETURNING {COLUMNS}"), &[&id, &now])
        .await?;
    Ok(Json(json!({ "trade": row_json(&row) })))
}

/// `DELETE /api/journal/{id}`: gone from the journal; its drawing, if it has one, stays on the chart.
pub async fn delete_one(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    match client.execute("DELETE FROM journal_trade WHERE id = $1 AND owner = $2", &[&id, &owner]).await? {
        0 => Err(ApiError::not_found(format!("trade {id} not found"))),
        _ => Ok(Json(json!({ "id": id }))),
    }
}

/// An ISO week: `2026-W42`.
fn valid_week(week: &str) -> bool {
    let Some((year, w)) = week.split_once("-W") else { return false };
    year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) && w.len() == 2 && w.parse::<u8>().is_ok_and(|n| (1..=53).contains(&n))
}

/// `GET /api/journal/reviews`: this browser's weekly reviews, by week.
pub async fn reviews(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let rows = client.query("SELECT week, answers, saved_at FROM journal_review WHERE owner = $1 ORDER BY week DESC", &[&owner]).await?;
    let list: Vec<Value> = rows.iter().map(|r| json!({ "week": r.get::<_, String>(0), "answers": r.get::<_, Value>(1), "savedAt": r.get::<_, i64>(2) })).collect();
    Ok(Json(json!({ "reviews": list })))
}

/// `PUT /api/journal/reviews/{week}` `{answers: {q1: "…", …}}`: a week's review, as it now stands.
pub async fn save_review(State(state): State<AppState>, headers: HeaderMap, Path(week): Path<String>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    if !valid_week(&week) {
        return Err(ApiError::bad_request("week is an ISO week, like 2026-W42"));
    }
    let answers = body.get("answers").and_then(Value::as_object).ok_or_else(|| ApiError::bad_request("answers must be an object"))?;
    if answers.len() > 20 || answers.iter().any(|(k, v)| k.len() > 8 || !v.is_string() || v.as_str().is_some_and(|s| s.len() > MAX_TEXT)) {
        return Err(ApiError::bad_request(format!("answers are a few short keys, each a string of at most {MAX_TEXT} bytes")));
    }
    let now = now_ms();
    let client = state.pool.get().await?;
    client
        .execute(
            "INSERT INTO journal_review (owner, week, answers, saved_at) VALUES ($1, $2, $3, $4)
             ON CONFLICT (owner, week) DO UPDATE SET answers = EXCLUDED.answers, saved_at = EXCLUDED.saved_at",
            &[&owner, &week, &Value::Object(answers.clone()), &now],
        )
        .await?;
    Ok(Json(json!({ "week": week, "savedAt": now })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 60 * MINUTE;

    fn pts(entry: f64, stop: f64, target: f64, time: f64) -> Vec<Point> {
        vec![Point { time, price: entry }, Point { time, price: stop }, Point { time: time + 10.0 * H as f64, price: target }]
    }

    fn bar(t: i64, h: f64, l: f64) -> Candle {
        candle(t, (h + l) / 2.0, h, l, (h + l) / 2.0)
    }

    #[test]
    fn a_position_risks_what_its_settings_say() {
        let style = |s: Value| s.as_object().unwrap().clone();
        assert_eq!(risk_of(&Map::new()), 10.0, "1% of a 1,000 account, as the chart draws it");
        assert_eq!(risk_of(&style(json!({ "account": "5000", "risk": 2 }))), 100.0);
        assert_eq!(risk_of(&style(json!({ "riskUnit": "USD", "risk": "25" }))), 25.0, "risk in cash");
        let plan = plan_of("long", &pts(100.0, 95.0, 110.0, 0.0), &Map::new(), 30 * H, "1h").unwrap();
        assert_eq!((plan.qty, plan.start_at), (2.0, 0), "ten risked over five a coin; a box wholly on bars past, weighed from its edge");
        assert!(plan_of("long", &pts(100.0, 105.0, 110.0, 0.0), &Map::new(), 0, "1h").is_none(), "a long's stop is under its entry");
        assert!(plan_of("trendline", &pts(100.0, 95.0, 110.0, 0.0), &Map::new(), 0, "1h").is_none());
    }

    #[test]
    fn a_position_reaching_the_present_is_weighed_from_when_it_was_drawn() {
        // Drawn half an hour into the 4h bar its edge sits on: that bar's earlier swings are not its own.
        let drawn = 4 * H + 30 * MINUTE;
        let plan = plan_of("short", &pts(100.0, 105.0, 90.0, (4 * H) as f64), &Map::new(), drawn, "4h").unwrap();
        assert_eq!(plan.start_at, drawn);
        // Its left edge twenty minutes back on a 1m chart, its box running on into the minutes to come: a trade taken now.
        let drawn = 100 * H;
        let plan = plan_of("long", &pts(100.0, 95.0, 110.0, (drawn - 20 * MINUTE) as f64), &Map::new(), drawn, "1m").unwrap();
        assert_eq!(plan.start_at, drawn);
    }

    fn trade(side: Side, entry: f64, stop: f64, target: f64) -> Trade {
        Trade { side, entry, stop, target, status: "planned".into(), opened_at: None, closed_at: None, exit: None, checked_until: 0 }
    }

    #[test]
    fn price_opens_a_trade_at_its_entry_and_closes_it_at_target_or_stop() {
        let mut t = trade(Side::Long, 100.0, 95.0, 110.0);
        t.walk(&[bar(0, 104.0, 101.0), bar(MINUTE, 102.0, 99.5), bar(2 * MINUTE, 108.0, 101.0), bar(3 * MINUTE, 111.0, 104.0), bar(4 * MINUTE, 120.0, 90.0)]);
        assert_eq!((t.status.as_str(), t.opened_at, t.closed_at, t.exit), ("win", Some(MINUTE), Some(3 * MINUTE), Some(110.0)));
        assert_eq!(t.checked_until, 4 * MINUTE, "nothing read past the close");
        let mut short = trade(Side::Short, 100.0, 105.0, 90.0);
        short.walk(&[bar(0, 100.5, 99.0), bar(MINUTE, 106.0, 98.0)]);
        assert_eq!((short.status.as_str(), short.exit), ("loss", Some(105.0)), "the short's stop is over it");
    }

    #[test]
    fn a_minute_reaching_both_counts_as_the_stop() {
        let mut t = trade(Side::Long, 100.0, 95.0, 110.0);
        t.walk(&[bar(0, 101.0, 99.0), bar(MINUTE, 112.0, 94.0)]);
        assert_eq!(t.status, "loss");
    }

    #[test]
    fn a_planned_trade_waits_for_its_entry_and_reads_on_from_where_it_stopped() {
        let mut t = trade(Side::Long, 90.0, 85.0, 100.0);
        t.walk(&[bar(0, 99.0, 95.0), bar(MINUTE, 101.0, 96.0)]);
        assert_eq!((t.status.as_str(), t.checked_until), ("planned", 2 * MINUTE), "the target reached before the entry is no trade");
        t.walk(&[bar(0, 99.0, 95.0), bar(MINUTE, 101.0, 96.0), bar(2 * MINUTE, 96.0, 89.0)]);
        assert_eq!((t.status.as_str(), t.opened_at), ("open", Some(2 * MINUTE)), "read again from the third minute only");
        // Price jumping over the entry between two minutes reaches it too.
        let mut gap = trade(Side::Short, 100.0, 110.0, 80.0);
        gap.walk(&[bar(0, 98.0, 97.0), bar(MINUTE, 103.0, 101.0)]);
        assert_eq!(gap.status, "open");
    }

    #[test]
    fn weeks_are_iso_weeks() {
        assert!(valid_week("2026-W42"));
        assert!(!valid_week("2026-W54") && !valid_week("2026-42") && !valid_week("26-W01"));
    }

    #[test]
    fn only_markets_whose_minutes_are_read_are_followed() {
        assert!(tracked("binance-perp:BTCUSDT") && tracked("hyperliquid-perp:SP500USD") && tracked("okx-spot:BTCUSDT"));
        assert!(!tracked("index:TOTAL") && !tracked("kraken-spot:BTCUSD") && !tracked("lighter-perp:ETHUSD"));
    }
}
