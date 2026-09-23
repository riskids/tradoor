//! Aggregated open interest: one coin's open interest on every venue that
//! lists it, summed, in coins.
//!
//! Only some exchanges publish an open interest history, and none of them for
//! long — Deribit and Hyperliquid publish none at all. So the server records
//! it: every few seconds it reads each venue's current figure and keeps one row
//! a minute, and where a venue does keep a five-minute history it backfills the
//! last month from it. A chart then asks for the sum over the venues it wants,
//! as candles of any step from a minute to an hour, and folds those into its
//! own bars.
//!
//! Everything is in coins of the chart's base, so venues quoting contracts or
//! dollars are converted here, and a `1000PEPE` chart counts thousands of PEPE
//! on every venue.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::api::{ApiError, ApiResult, AppState};
use crate::liquidations::now_ms;

const MINUTE: i64 = 60_000;
const FIVE_MINUTES: i64 = 5 * MINUTE;
/// How often every venue is read.
const POLL: Duration = Duration::from_secs(15);
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
/// Rows a minute apart are kept a week, then only those on five minutes for
/// the month, then only those on the hour — plenty for the longer charts that
/// reach back, at a fraction of the rows.
const KEEP_MINUTES_MS: i64 = 7 * DAY;
const KEEP_MS: i64 = 35 * DAY;
const KEEP_HOURLY_MS: i64 = 400 * DAY;
/// Binance keeps thirty days; asking for the very edge of that is refused.
const BACKFILL_MS: i64 = 29 * 86_400_000;
/// A venue that does not list a coin is asked again after this long.
const UNLISTED_RETRY_MS: i64 = 10 * MINUTE;
/// Past this long without a reading, a venue's last figure no longer counts.
const STALE_MS: i64 = 2 * 3_600_000;
/// Coins a chart can start recording, beyond the configured ones.
const MAX_COINS: usize = 40;
/// Steps a chart may ask for; each divides the next, and an hour divides every bar.
const STEPS: [i64; 5] = [MINUTE, FIVE_MINUTES, 15 * MINUTE, 30 * MINUTE, 60 * MINUTE];
/// The most candles one request answers with.
const MAX_BUCKETS: i64 = 20_000;

// ---------------------------------------------------------------- venues

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OiVenue {
    BinanceUsdt,
    BybitUsdt,
    OkxUsdt,
    Deribit,
    Hyperliquid,
    BinanceUsd,
    BybitUsd,
    OkxUsd,
}

impl OiVenue {
    /// Velo's order: stablecoin-margined first, then the coin-margined contracts.
    pub const ALL: [OiVenue; 8] = [Self::BinanceUsdt, Self::BybitUsdt, Self::OkxUsdt, Self::Deribit, Self::Hyperliquid, Self::BinanceUsd, Self::BybitUsd, Self::OkxUsd];

    pub fn id(self) -> &'static str {
        match self {
            Self::BinanceUsdt => "binance-usdt",
            Self::BybitUsdt => "bybit-usdt",
            Self::OkxUsdt => "okx-usdt",
            Self::Deribit => "deribit-usd",
            Self::Hyperliquid => "hyperliquid-usd",
            Self::BinanceUsd => "binance-usd",
            Self::BybitUsd => "bybit-usd",
            Self::OkxUsd => "okx-usd",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::BinanceUsdt => "Binance USDT Perp",
            Self::BybitUsdt => "Bybit USDT Perp",
            Self::OkxUsdt => "OKX USDT Swap",
            Self::Deribit => "Deribit USD Perp",
            Self::Hyperliquid => "Hyperliquid USD Perp",
            Self::BinanceUsd => "Binance USD Perp",
            Self::BybitUsd => "Bybit USD Perp",
            Self::OkxUsd => "OKX USD Perp",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == id)
    }

    /// Whether the exchange keeps a five-minute history to backfill from.
    /// Binance's coin-margined history disagrees with its live figure by more
    /// than double, so it is recorded from now on only.
    pub fn has_history(self) -> bool {
        matches!(self, Self::BinanceUsdt | Self::BybitUsdt | Self::OkxUsdt | Self::BybitUsd | Self::OkxUsd)
    }

    /// How far back the exchange keeps an HOURLY history, beyond its
    /// five-minute one: Bybit a year and more, OKX about seven weeks, Binance
    /// nothing past its thirty days.
    pub fn hourly_reach(self) -> Option<i64> {
        match self {
            Self::BybitUsdt | Self::BybitUsd => Some(365 * DAY),
            Self::OkxUsdt | Self::OkxUsd => Some(55 * DAY),
            _ => None,
        }
    }
}

/// A coin as a chart names it, and the bare coin behind a multiplier:
/// `1000PEPE` is PEPE counted in thousands.
#[derive(Clone, Debug, PartialEq)]
pub struct Coin {
    pub name: String,
    pub bare: String,
    pub mult: f64,
}

impl Coin {
    pub fn parse(name: &str) -> Option<Self> {
        if name.is_empty() || name.len() > 20 || !name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
            return None;
        }
        let digits = name.bytes().take_while(u8::is_ascii_digit).count();
        let (prefix, rest) = name.split_at(digits);
        // Only a power of ten of a hundred and up is a multiplier: 1INCH is a coin.
        let mult = (prefix.len() >= 3 && prefix.starts_with('1') && prefix[1..].bytes().all(|b| b == b'0') && !rest.is_empty()).then(|| 10f64.powi(prefix.len() as i32 - 1));
        Some(match mult {
            Some(m) => Self { name: name.to_string(), bare: rest.to_string(), mult: m },
            None => Self { name: name.to_string(), bare: name.to_string(), mult: 1.0 },
        })
    }

    /// Hyperliquid spells a thousand-coin `kPEPE`.
    fn hyperliquid(&self) -> Option<String> {
        match self.mult {
            m if m == 1.0 => Some(self.bare.clone()),
            m if m == 1000.0 => Some(format!("k{}", self.bare)),
            _ => None,
        }
    }
}

const BINANCE: &str = "https://fapi.binance.com";
const BINANCE_COIN: &str = "https://dapi.binance.com";
const BYBIT: &str = "https://api.bybit.com";
const OKX: &str = "https://www.okx.com";
const DERIBIT: &str = "https://www.deribit.com";
const HYPERLIQUID: &str = "https://api.hyperliquid.xyz/info";

/// Numbers arrive as strings or numbers.
fn num(v: &Value) -> Option<f64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_f64()).filter(|n: &f64| n.is_finite())
}

fn int(v: &Value) -> Option<i64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_i64())
}

pub fn parse_binance_current(v: &Value) -> Option<f64> {
    num(&v["openInterest"])
}

/// Bybit's `openInterest` counts both sides now; `singleOpenInterest` is the
/// one-sided figure every other exchange reports.
pub fn parse_bybit_ticker(v: &Value) -> Option<(f64, f64)> {
    let t = &v["result"]["list"][0];
    let oi = num(&t["singleOpenInterest"]).or_else(|| num(&t["openInterest"]).map(|o| o / 2.0))?;
    Some((oi, num(&t["markPrice"]).unwrap_or(f64::NAN)))
}

pub fn parse_okx_current(v: &Value) -> Option<f64> {
    num(&v["data"][0]["oiCcy"])
}

/// Deribit's perpetual is counted in dollars.
pub fn parse_deribit(v: &Value) -> Option<f64> {
    let r = &v["result"][0];
    let (usd, mark) = (num(&r["open_interest"])?, num(&r["mark_price"])?);
    (mark > 0.0).then(|| usd / mark)
}

pub fn parse_hyperliquid(v: &Value, name: &str) -> Option<f64> {
    let universe = v[0]["universe"].as_array()?;
    let i = universe.iter().position(|u| u["name"] == name)?;
    num(&v[1][i]["openInterest"])
}

/// Binance's coin-margined contracts are $100 for BTC and $10 for the rest.
pub fn parse_binance_coin(oi: &Value, premium: &Value, bare: &str) -> Option<f64> {
    let contracts = num(&oi["openInterest"])?;
    let mark = num(&premium[0]["markPrice"]).or_else(|| num(&premium["markPrice"]))?;
    let size = if bare == "BTC" { 100.0 } else { 10.0 };
    (mark > 0.0).then(|| contracts * size / mark)
}

/// `[{timestamp, sumOpenInterest}]`, oldest first.
pub fn parse_binance_history(v: &Value) -> Vec<(i64, f64)> {
    v.as_array().map(|rows| rows.iter().filter_map(|r| Some((int(&r["timestamp"])?, num(&r["sumOpenInterest"])?))).collect()).unwrap_or_default()
}

/// Bybit's history is newest first, one-sided in `singleOpenInterest`.
pub fn parse_bybit_history(v: &Value) -> Vec<(i64, f64)> {
    let mut out: Vec<(i64, f64)> = v["result"]["list"]
        .as_array()
        .map(|rows| rows.iter().filter_map(|r| Some((int(&r["timestamp"])?, num(&r["singleOpenInterest"]).or_else(|| num(&r["openInterest"]).map(|o| o / 2.0))?))).collect())
        .unwrap_or_default();
    out.sort_by_key(|p| p.0);
    out
}

/// `[[ts, oi, oiCcy, oiUsd]]`, newest first; the coins are the third.
pub fn parse_okx_history(v: &Value) -> Vec<(i64, f64)> {
    let mut out: Vec<(i64, f64)> = v["data"].as_array().map(|rows| rows.iter().filter_map(|r| Some((int(&r[0])?, num(&r[2])?))).collect()).unwrap_or_default();
    out.sort_by_key(|p| p.0);
    out
}

/// Bybit mark-price klines, `[[start, o, h, l, c]]`: the close of each five minutes.
pub fn parse_bybit_marks(v: &Value) -> HashMap<i64, f64> {
    v["result"]["list"].as_array().map(|rows| rows.iter().filter_map(|r| Some((int(&r[0])?, num(&r[4])?))).collect()).unwrap_or_default()
}

// ---------------------------------------------------------------- fetching

pub struct Http {
    client: reqwest::Client,
}

impl Http {
    pub fn new(doh: &Arc<crate::dns::Doh>) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder().dns_resolver(doh.resolver()).user_agent(concat!("tradoor/", env!("CARGO_PKG_VERSION"))).timeout(Duration::from_secs(15)).build()?;
        Ok(Self { client })
    }

    async fn json(&self, req: reqwest::RequestBuilder) -> Result<Value, String> {
        let resp = req.send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("{status}: {}", body.chars().take(160).collect::<String>()));
        }
        serde_json::from_str(&body).map_err(|e| e.to_string())
    }

    async fn get(&self, url: &str) -> Result<Value, String> {
        self.json(self.client.get(url)).await
    }

    async fn post(&self, url: &str, body: &Value) -> Result<Value, String> {
        self.json(self.client.post(url).header("content-type", "application/json").body(body.to_string())).await
    }
}

/// What a read came back with: the figure, or why there is none — `Unlisted`
/// when the venue has no such contract, so it is not asked every few seconds.
#[derive(Debug, PartialEq)]
pub enum Reading {
    Coins(f64),
    Unlisted,
    Failed(String),
}

fn reading(result: Result<Option<f64>, String>) -> Reading {
    match result {
        Ok(Some(v)) if v.is_finite() && v >= 0.0 => Reading::Coins(v),
        Ok(_) => Reading::Unlisted,
        // Exchanges answer an unknown symbol with a 400 and their own message.
        Err(e) if e.starts_with("400") => Reading::Unlisted,
        Err(e) => Reading::Failed(e),
    }
}

async fn current(http: &Http, venue: OiVenue, coin: &Coin, hyperliquid: &Option<Value>) -> Reading {
    let (name, bare) = (&coin.name, &coin.bare);
    let result: Result<Option<f64>, String> = async {
        Ok(match venue {
            OiVenue::BinanceUsdt => parse_binance_current(&http.get(&format!("{BINANCE}/fapi/v1/openInterest?symbol={name}USDT")).await?),
            OiVenue::BybitUsdt => {
                let v = http.get(&format!("{BYBIT}/v5/market/tickers?category=linear&symbol={name}USDT")).await?;
                if v["retCode"] != 0 {
                    return Ok(None);
                }
                parse_bybit_ticker(&v).map(|(oi, _)| oi)
            }
            OiVenue::OkxUsdt => parse_okx_current(&http.get(&format!("{OKX}/api/v5/public/open-interest?instType=SWAP&instId={bare}-USDT-SWAP")).await?).map(|c| c / coin.mult),
            OiVenue::Deribit => {
                if coin.mult != 1.0 || !matches!(bare.as_str(), "BTC" | "ETH") {
                    return Ok(None);
                }
                parse_deribit(&http.get(&format!("{DERIBIT}/api/v2/public/get_book_summary_by_instrument?instrument_name={bare}-PERPETUAL")).await?)
            }
            OiVenue::Hyperliquid => match (coin.hyperliquid(), hyperliquid) {
                (Some(n), Some(v)) => parse_hyperliquid(v, &n),
                (Some(_), None) => return Err("Hyperliquid did not answer".into()),
                (None, _) => None,
            },
            OiVenue::BinanceUsd => {
                if coin.mult != 1.0 {
                    return Ok(None);
                }
                let (oi_url, premium_url) = (format!("{BINANCE_COIN}/dapi/v1/openInterest?symbol={bare}USD_PERP"), format!("{BINANCE_COIN}/dapi/v1/premiumIndex?symbol={bare}USD_PERP"));
                let (oi, premium) = tokio::join!(http.get(&oi_url), http.get(&premium_url));
                parse_binance_coin(&oi?, &premium?, bare)
            }
            OiVenue::BybitUsd => {
                if coin.mult != 1.0 {
                    return Ok(None);
                }
                let v = http.get(&format!("{BYBIT}/v5/market/tickers?category=inverse&symbol={bare}USD")).await?;
                if v["retCode"] != 0 {
                    return Ok(None);
                }
                parse_bybit_ticker(&v).and_then(|(usd, mark)| (mark > 0.0).then(|| usd / mark))
            }
            OiVenue::OkxUsd => {
                if coin.mult != 1.0 {
                    return Ok(None);
                }
                parse_okx_current(&http.get(&format!("{OKX}/api/v5/public/open-interest?instType=SWAP&instId={bare}-USD-SWAP")).await?)
            }
        })
    }
    .await;
    reading(result)
}

/// History from `from` up to `to`, oldest first, paged backwards: five-minute
/// points, or hourly ones where `hourly`.
async fn history(http: &Http, venue: OiVenue, coin: &Coin, from: i64, to: i64, hourly: bool) -> Result<Vec<(i64, f64)>, String> {
    let (name, bare) = (&coin.name, &coin.bare);
    let step = if hourly { HOUR } else { FIVE_MINUTES };
    let (bybit_interval, mark_interval, okx_period) = if hourly { ("1h", "60", "1H") } else { ("5min", "5", "5m") };
    let mut out: Vec<(i64, f64)> = Vec::new();
    let mut end = to;
    for _ in 0..80 {
        if end <= from {
            break;
        }
        let page = match venue {
            OiVenue::BinanceUsdt => {
                let start = (end - 500 * step).max(from);
                let period = if hourly { "1h" } else { "5m" };
                parse_binance_history(&http.get(&format!("{BINANCE}/futures/data/openInterestHist?symbol={name}USDT&period={period}&limit=500&startTime={start}&endTime={end}")).await?)
            }
            OiVenue::BybitUsdt | OiVenue::BybitUsd => {
                let (category, symbol) = if venue == OiVenue::BybitUsdt { ("linear", format!("{name}USDT")) } else { ("inverse", format!("{bare}USD")) };
                let start = (end - 199 * step).max(from);
                let oi = parse_bybit_history(&http.get(&format!("{BYBIT}/v5/market/open-interest?category={category}&symbol={symbol}&intervalTime={bybit_interval}&limit=200&startTime={start}&endTime={end}")).await?);
                if venue == OiVenue::BybitUsd {
                    // Inverse open interest is in dollars: the mark price of the same moment makes it coins.
                    let marks = parse_bybit_marks(&http.get(&format!("{BYBIT}/v5/market/mark-price-kline?category=inverse&symbol={symbol}&interval={mark_interval}&limit=1000&start={start}&end={end}")).await?);
                    oi.into_iter().filter_map(|(t, usd)| marks.get(&t).filter(|m| **m > 0.0).map(|m| (t, usd / m))).collect()
                } else {
                    oi
                }
            }
            OiVenue::OkxUsdt | OiVenue::OkxUsd => {
                let inst = if venue == OiVenue::OkxUsdt { format!("{bare}-USDT-SWAP") } else { format!("{bare}-USD-SWAP") };
                parse_okx_history(&http.get(&format!("{OKX}/api/v5/rubik/stat/contracts/open-interest-history?instId={inst}&period={okx_period}&limit=100&end={end}")).await?).into_iter().map(|(t, c)| (t, c / coin.mult)).collect()
            }
            _ => return Ok(Vec::new()),
        };
        let Some(&(oldest, _)) = page.first() else { break };
        out.extend(page.into_iter().filter(|(t, _)| *t >= from && *t < to));
        if oldest >= end {
            break;
        }
        end = oldest - 1;
        // Gentle on the exchanges' weights: a backfill is not urgent.
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    out.sort_by_key(|p| p.0);
    out.dedup_by_key(|p| p.0);
    Ok(out)
}

// ---------------------------------------------------------------- recorder

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VenueHealth {
    pub venue: &'static str,
    /// `ok`, `unlisted` or `failing`.
    pub state: &'static str,
    pub error: Option<String>,
    #[serde(skip)]
    unlisted_at: i64,
}

pub struct Recorder {
    pool: deadpool_postgres::Pool,
    http: Http,
    coins: Mutex<Vec<String>>,
    /// Per coin, each venue's last outcome.
    health: Mutex<HashMap<String, Vec<VenueHealth>>>,
}

impl Recorder {
    /// Starts recording `configured` and every coin a chart asked for before.
    pub async fn start(pool: deadpool_postgres::Pool, configured: Vec<String>, doh: &Arc<crate::dns::Doh>) -> anyhow::Result<Arc<Self>> {
        let client = pool.get().await?;
        for coin in &configured {
            client.execute("INSERT INTO open_interest_coin (coin) VALUES ($1) ON CONFLICT DO NOTHING", &[coin]).await?;
        }
        let coins: Vec<String> = client.query("SELECT coin FROM open_interest_coin ORDER BY added_at", &[]).await?.iter().map(|r| r.get(0)).collect();
        drop(client);
        let recorder = Arc::new(Self { pool, http: Http::new(doh)?, coins: Mutex::new(coins.clone()), health: Mutex::new(HashMap::new()) });
        for coin in coins {
            tokio::spawn(recorder.clone().backfill(coin));
        }
        tokio::spawn(recorder.clone().poll());
        tokio::spawn(recorder.clone().trim());
        tracing::info!(coins = ?recorder.coins(), "recording open interest");
        Ok(recorder)
    }

    pub fn coins(&self) -> Vec<String> {
        self.coins.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Starts recording a coin a chart asked for. False when the list is full.
    pub async fn track(self: &Arc<Self>, coin: &str) -> bool {
        {
            let Ok(mut coins) = self.coins.lock() else { return false };
            if coins.iter().any(|c| c == coin) {
                return true;
            }
            if coins.len() >= MAX_COINS {
                return false;
            }
            coins.push(coin.to_string());
        }
        if let Ok(client) = self.pool.get().await {
            let _ = client.execute("INSERT INTO open_interest_coin (coin) VALUES ($1) ON CONFLICT DO NOTHING", &[&coin]).await;
        }
        tokio::spawn(self.clone().backfill(coin.to_string()));
        true
    }

    pub fn health(&self, coin: &str) -> Vec<VenueHealth> {
        self.health.lock().ok().and_then(|h| h.get(coin).cloned()).unwrap_or_default()
    }

    async fn poll(self: Arc<Self>) {
        let mut every = tokio::time::interval(POLL);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            let now = now_ms();
            // One Hyperliquid call covers every coin.
            let hyperliquid = self.http.post(HYPERLIQUID, &json!({ "type": "metaAndAssetCtxs" })).await.ok();
            let hyperliquid = Arc::new(hyperliquid);
            let mut rows: Vec<(String, &'static str, f64)> = Vec::new();
            for name in self.coins() {
                let Some(coin) = Coin::parse(&name) else { continue };
                let skip: Vec<&'static str> = self.health(&name).iter().filter(|h| h.state == "unlisted" && now - h.unlisted_at < UNLISTED_RETRY_MS).map(|h| h.venue).collect();
                let mut reads = JoinSet::new();
                for venue in OiVenue::ALL.into_iter().filter(|v| !skip.contains(&v.id())) {
                    let (recorder, coin, hl) = (self.clone(), coin.clone(), hyperliquid.clone());
                    reads.spawn(async move { (venue, current(&recorder.http, venue, &coin, &hl).await) });
                }
                let mut health: Vec<VenueHealth> = self.health(&name).into_iter().filter(|h| skip.contains(&h.venue)).collect();
                while let Some(Ok((venue, read))) = reads.join_next().await {
                    let (state, error, unlisted_at) = match read {
                        Reading::Coins(v) => {
                            rows.push((name.clone(), venue.id(), v));
                            ("ok", None, 0)
                        }
                        Reading::Unlisted => ("unlisted", None, now),
                        Reading::Failed(e) => ("failing", Some(e), 0),
                    };
                    health.push(VenueHealth { venue: venue.id(), state, error, unlisted_at });
                }
                health.sort_by_key(|h| OiVenue::ALL.iter().position(|v| v.id() == h.venue));
                if let Ok(mut all) = self.health.lock() {
                    all.insert(name.clone(), health);
                }
            }
            if let Err(e) = self.write(&rows, now - now.rem_euclid(MINUTE), true).await {
                tracing::error!(error = %e, "writing open interest");
            }
        }
    }

    /// Rows at one time; the current minute's row is overwritten by each read.
    async fn write(&self, rows: &[(String, &'static str, f64)], time: i64, overwrite: bool) -> Result<(), String> {
        if rows.is_empty() {
            return Ok(());
        }
        let client = self.pool.get().await.map_err(|e| e.to_string())?;
        let coins: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        let venues: Vec<&str> = rows.iter().map(|r| r.1).collect();
        let values: Vec<f64> = rows.iter().map(|r| r.2).collect();
        let times = vec![time; rows.len()];
        let conflict = if overwrite { "DO UPDATE SET oi = EXCLUDED.oi" } else { "DO NOTHING" };
        client
            .execute(
                &format!("INSERT INTO open_interest (coin, venue, time, oi) SELECT * FROM UNNEST($1::text[], $2::text[], $3::int8[], $4::float8[]) ON CONFLICT (coin, venue, time) {conflict}"),
                &[&coins, &venues, &times, &values],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Fill what the exchanges still hold and this table does not: the month
    /// before recording began, and any gap since it last ran.
    async fn backfill(self: Arc<Self>, name: String) {
        let Some(coin) = Coin::parse(&name) else { return };
        let now = now_ms();
        for venue in OiVenue::ALL.into_iter().filter(|v| v.has_history()) {
            let span: Option<(Option<i64>, Option<i64>)> = async {
                let client = self.pool.get().await.ok()?;
                let row = client.query_one("SELECT MIN(time), MAX(time) FROM open_interest WHERE coin = $1 AND venue = $2", &[&name, &venue.id()]).await.ok()?;
                Some((row.get(0), row.get(1)))
            }
            .await;
            let Some((first, last)) = span else { continue };
            let from = now - BACKFILL_MS;
            let mut ranges = Vec::new();
            match (first, last) {
                (Some(first), Some(last)) => {
                    if first > from + FIVE_MINUTES {
                        ranges.push((from, first));
                    }
                    if now - last > 10 * MINUTE {
                        ranges.push((last + 1, now));
                    }
                }
                _ => ranges.push((from, now)),
            }
            for (a, b) in ranges {
                self.fill(&coin, venue, a, b, false).await;
            }
            // Past the five-minute history, the hourly one, back as far as the exchange keeps it.
            if let Some(reach) = venue.hourly_reach() {
                let first = async {
                    let client = self.pool.get().await.ok()?;
                    client.query_one("SELECT MIN(time) FROM open_interest WHERE coin = $1 AND venue = $2", &[&name, &venue.id()]).await.ok()?.get::<_, Option<i64>>(0)
                }
                .await;
                let deep = now - reach;
                if let Some(first) = first.filter(|f| *f > deep + HOUR) {
                    self.fill(&coin, venue, deep, first - first.rem_euclid(HOUR), true).await;
                }
            }
        }
    }

    /// Fetch one stretch of history and store it under what is already there.
    async fn fill(&self, coin: &Coin, venue: OiVenue, from: i64, to: i64, hourly: bool) {
        let name = &coin.name;
        match history(&self.http, venue, coin, from, to, hourly).await {
            Ok(points) if !points.is_empty() => {
                let n = points.len();
                for chunk in points.chunks(2_000) {
                    let rows: Vec<(String, &'static str, f64)> = chunk.iter().map(|(_, v)| (name.clone(), venue.id(), *v)).collect();
                    let times: Vec<i64> = chunk.iter().map(|(t, _)| *t).collect();
                    if let Err(e) = self.write_at(&rows, &times).await {
                        tracing::warn!(coin = %name, venue = venue.id(), error = %e, "backfill write failed");
                    }
                }
                tracing::info!(coin = %name, venue = venue.id(), points = n, hourly, "open interest backfilled");
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(coin = %name, venue = venue.id(), error = %e, hourly, "open interest backfill failed"),
        }
    }

    /// Backfilled rows, each at its own time, never over a recorded one.
    async fn write_at(&self, rows: &[(String, &'static str, f64)], times: &[i64]) -> Result<(), String> {
        let client = self.pool.get().await.map_err(|e| e.to_string())?;
        let coins: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        let venues: Vec<&str> = rows.iter().map(|r| r.1).collect();
        let values: Vec<f64> = rows.iter().map(|r| r.2).collect();
        client
            .execute(
                "INSERT INTO open_interest (coin, venue, time, oi) SELECT * FROM UNNEST($1::text[], $2::text[], $3::int8[], $4::float8[]) ON CONFLICT (coin, venue, time) DO NOTHING",
                &[&coins, &venues, &times, &values],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn trim(self: Arc<Self>) {
        let mut every = tokio::time::interval(Duration::from_secs(3_600));
        loop {
            every.tick().await;
            if let Ok(client) = self.pool.get().await {
                let now = now_ms();
                let sql = "DELETE FROM open_interest WHERE (time < $1 AND time % 300000 <> 0) OR (time < $2 AND time % 3600000 <> 0) OR time < $3";
                if let Err(e) = client.execute(sql, &[&(now - KEEP_MINUTES_MS), &(now - KEEP_MS), &(now - KEEP_HOURLY_MS)]).await {
                    tracing::error!(error = %e, "trimming open interest");
                }
            }
        }
    }
}

// ---------------------------------------------------------------- sums

/// One venue's candle in one bucket.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ohlc {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
}

/// The venues' candles summed bucket by bucket. A venue with no reading in a
/// bucket carries its last close forward, for as long as that is recent.
///
/// Venues did not all start at once — Deribit and Hyperliquid only from when
/// this server began recording them — and simply adding one where it starts
/// would draw a cliff that is not in the market. So the venues present at the
/// first bucket are the base, and a venue that joins later is estimated before
/// its first reading at the share of the base it held when it joined.
/// `seed` is each venue's last reading before the first bucket.
pub fn aggregate(rows: &[(String, i64, Ohlc)], seed: &HashMap<String, (i64, f64)>) -> Vec<(i64, Ohlc)> {
    let mut times: Vec<i64> = rows.iter().map(|r| r.1).collect();
    times.sort_unstable();
    times.dedup();
    if times.is_empty() {
        return Vec::new();
    }
    let mut names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).chain(seed.keys().map(String::as_str)).collect();
    names.sort_unstable();
    names.dedup();
    let at: HashMap<i64, usize> = times.iter().enumerate().map(|(i, t)| (*t, i)).collect();

    // Each venue's candle in each bucket, its last close carried while recent.
    let mut grid: Vec<Vec<Option<Ohlc>>> = vec![vec![None; times.len()]; names.len()];
    for (venue, bucket, ohlc) in rows {
        let v = names.iter().position(|n| n == venue).expect("every row's venue is named");
        grid[v][at[bucket]] = Some(*ohlc);
    }
    for (v, name) in names.iter().enumerate() {
        let mut last = seed.get(*name).copied();
        for (b, &t) in times.iter().enumerate() {
            match grid[v][b] {
                Some(o) => last = Some((t, o.close)),
                None => {
                    if let Some((_, close)) = last.filter(|(when, _)| t - when <= STALE_MS) {
                        grid[v][b] = Some(Ohlc { open: close, high: close, low: close, close });
                    }
                }
            }
        }
    }

    let add = |a: Ohlc, b: Ohlc| Ohlc { open: a.open + b.open, high: a.high + b.high, low: a.low + b.low, close: a.close + b.close };
    let zero = Ohlc { open: 0.0, high: 0.0, low: 0.0, close: 0.0 };
    // The base: whoever is there first. Buckets before anyone is there have no sum.
    let Some(first) = (0..times.len()).find(|&b| grid.iter().any(|g| g[b].is_some())) else { return Vec::new() };
    let base: Vec<usize> = (0..names.len()).filter(|&v| grid[v][first].is_some()).collect();
    let base_sum: Vec<Ohlc> = (0..times.len()).map(|b| base.iter().filter_map(|&v| grid[v][b]).fold(zero, add)).collect();
    // Each later venue's share of the base where it joined.
    let shares: Vec<(usize, usize, f64)> = (0..names.len())
        .filter(|v| !base.contains(v))
        .filter_map(|v| {
            let joined = (0..times.len()).find(|&b| grid[v][b].is_some())?;
            let base_close = base_sum[joined].close;
            (base_close > 0.0).then(|| (v, joined, grid[v][joined].map_or(0.0, |o| o.close) / base_close))
        })
        .collect();

    (first..times.len())
        .map(|b| {
            let mut sum = base_sum[b];
            for &(v, joined, share) in &shares {
                let part = match grid[v][b] {
                    Some(o) => o,
                    None if b < joined => Ohlc { open: base_sum[b].open * share, high: base_sum[b].high * share, low: base_sum[b].low * share, close: base_sum[b].close * share },
                    None => zero,
                };
                sum = add(sum, part);
            }
            (times[b], sum)
        })
        .collect()
}

#[derive(Deserialize)]
pub struct Params {
    coin: String,
    step: i64,
    from: i64,
    to: Option<i64>,
    /// Venue ids, comma separated; every venue when absent.
    venues: Option<String>,
}

/// `GET /api/open-interest?coin=BTC&step=3600000&from=…&venues=binance-usdt,…`:
/// the summed open interest as candles of `step`, oldest first, in coins.
pub async fn candles(State(state): State<AppState>, Query(p): Query<Params>) -> ApiResult<Json<Value>> {
    let recorder = state.open_interest.as_ref().ok_or_else(|| ApiError::not_found("open interest is not being recorded on this server"))?;
    let coin = Coin::parse(&p.coin).ok_or_else(|| ApiError::bad_request("a coin is capital letters and digits, like BTC or 1000PEPE"))?;
    if !STEPS.contains(&p.step) {
        return Err(ApiError::bad_request("step is one of 60000, 300000, 900000, 1800000 or 3600000 ms"));
    }
    let to = p.to.unwrap_or_else(now_ms);
    let from = p.from - p.from.rem_euclid(p.step);
    if to <= from || (to - from) / p.step > MAX_BUCKETS {
        return Err(ApiError::bad_request(format!("from..to must span 1 to {MAX_BUCKETS} steps")));
    }
    let venues: Vec<OiVenue> = match &p.venues {
        Some(list) => list.split(',').filter(|s| !s.is_empty()).map(|s| OiVenue::parse(s).ok_or_else(|| ApiError::bad_request(format!("unknown venue: {s}")))).collect::<Result<_, _>>()?,
        None => OiVenue::ALL.to_vec(),
    };
    let ids: Vec<&str> = venues.iter().map(|v| v.id()).collect();
    // Recording starts only for a coin some exchange lists a perpetual on: the
    // list is capped, and made-up names would fill it for good.
    let tracked = if recorder.coins().contains(&coin.name) {
        true
    } else if state.catalog.lists_perp(&coin.name).await {
        recorder.track(&coin.name).await
    } else {
        return Err(ApiError::not_found(format!("no exchange lists a {} perpetual", coin.name)));
    };

    let client = state.pool.get().await?;
    let rows = client
        .query(
            "SELECT venue, (time / $5) * $5 AS b,
                    (array_agg(oi ORDER BY time))[1], MAX(oi), MIN(oi), (array_agg(oi ORDER BY time DESC))[1]
             FROM open_interest
             WHERE coin = $1 AND venue = ANY($2) AND time >= $3 AND time < $4
             GROUP BY venue, b ORDER BY b",
            &[&coin.name, &ids, &from, &to, &p.step],
        )
        .await?;
    let rows: Vec<(String, i64, Ohlc)> = rows.iter().map(|r| (r.get(0), r.get(1), Ohlc { open: r.get(2), high: r.get(3), low: r.get(4), close: r.get(5) })).collect();
    let seed: HashMap<String, (i64, f64)> = client
        .query(
            "SELECT DISTINCT ON (venue) venue, time, oi FROM open_interest
             WHERE coin = $1 AND venue = ANY($2) AND time < $3 AND time >= $4
             ORDER BY venue, time DESC",
            &[&coin.name, &ids, &from, &(from - STALE_MS)],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), (r.get::<_, i64>(1) - r.get::<_, i64>(1).rem_euclid(p.step), r.get(2))))
        .collect();
    let since: HashMap<String, i64> = client
        .query("SELECT venue, MIN(time) FROM open_interest WHERE coin = $1 GROUP BY venue", &[&coin.name])
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    let sums = aggregate(&rows, &seed);

    let health = recorder.health(&coin.name);
    Ok(Json(json!({
        "coin": coin.name,
        "step": p.step,
        "tracked": tracked,
        "venues": OiVenue::ALL.iter().map(|v| {
            let h = health.iter().find(|h| h.venue == v.id());
            json!({
                "id": v.id(),
                "label": v.label(),
                "since": since.get(v.id()),
                "state": h.map(|h| h.state),
                "error": h.and_then(|h| h.error.clone()),
            })
        }).collect::<Vec<_>>(),
        "time": sums.iter().map(|s| s.0).collect::<Vec<_>>(),
        "open": sums.iter().map(|s| s.1.open).collect::<Vec<_>>(),
        "high": sums.iter().map(|s| s.1.high).collect::<Vec<_>>(),
        "low": sums.iter().map(|s| s.1.low).collect::<Vec<_>>(),
        "close": sums.iter().map(|s| s.1.close).collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coins_and_multipliers() {
        assert_eq!(Coin::parse("BTC").unwrap(), Coin { name: "BTC".into(), bare: "BTC".into(), mult: 1.0 });
        assert_eq!(Coin::parse("1000PEPE").unwrap().mult, 1000.0);
        assert_eq!(Coin::parse("1000PEPE").unwrap().hyperliquid().as_deref(), Some("kPEPE"));
        assert_eq!(Coin::parse("1INCH").unwrap().bare, "1INCH");
        assert_eq!(Coin::parse("1000000MOG").unwrap().mult, 1_000_000.0);
        assert!(Coin::parse("btc").is_none());
        assert!(Coin::parse("BTC-USD").is_none());
    }

    // Captured 2026-09-23.
    #[test]
    fn current_readings_are_one_sided_coins() {
        let v: Value = serde_json::from_str(r#"{"symbol":"BTCUSDT","openInterest":"106186.376","time":1790140941065}"#).unwrap();
        assert_eq!(parse_binance_current(&v), Some(106_186.376));
        let v: Value = serde_json::from_str(r#"{"retCode":0,"result":{"list":[{"symbol":"BTCUSDT","openInterest":"61800.723","singleOpenInterest":"30900.362","markPrice":"86800.00"}]}}"#).unwrap();
        assert_eq!(parse_bybit_ticker(&v), Some((30_900.362, 86_800.0)));
        let v: Value = serde_json::from_str(r#"{"code":"0","data":[{"instId":"BTC-USDT-SWAP","oi":"3001119.97","oiCcy":"30011.1997","oiUsd":"2603498584.05"}]}"#).unwrap();
        assert_eq!(parse_okx_current(&v), Some(30_011.1997));
        let v: Value = serde_json::from_str(r#"{"result":[{"instrument_name":"BTC-PERPETUAL","open_interest":851127620,"mark_price":86829.05}]}"#).unwrap();
        assert!((parse_deribit(&v).unwrap() - 9802.337).abs() < 0.01);
        let v: Value = serde_json::from_str(r#"[{"universe":[{"name":"BTC"},{"name":"kPEPE"}]},[{"openInterest":"46244.4424"},{"openInterest":"9"}]]"#).unwrap();
        assert_eq!((parse_hyperliquid(&v, "BTC"), parse_hyperliquid(&v, "kPEPE"), parse_hyperliquid(&v, "ETH")), (Some(46_244.4424), Some(9.0), None));
        let oi: Value = serde_json::from_str(r#"{"symbol":"BTCUSD_PERP","openInterest":"12102006"}"#).unwrap();
        let premium: Value = serde_json::from_str(r#"[{"symbol":"BTCUSD_PERP","markPrice":"86744.89"}]"#).unwrap();
        assert!((parse_binance_coin(&oi, &premium, "BTC").unwrap() - 13_951.26).abs() < 0.01);
    }

    #[test]
    fn histories_come_back_oldest_first() {
        let v: Value = serde_json::from_str(r#"[{"sumOpenInterest":"106275.503","timestamp":1790140500000},{"sumOpenInterest":"106310.407","timestamp":1790140800000}]"#).unwrap();
        assert_eq!(parse_binance_history(&v), vec![(1_790_140_500_000, 106_275.503), (1_790_140_800_000, 106_310.407)]);
        let v: Value = serde_json::from_str(r#"{"retCode":0,"result":{"list":[{"openInterest":"61846.748","singleOpenInterest":"30923.374","timestamp":"1790140800000"},{"openInterest":"61781.966","singleOpenInterest":"30890.983","timestamp":"1790140500000"}]}}"#).unwrap();
        assert_eq!(parse_bybit_history(&v), vec![(1_790_140_500_000, 30_890.983), (1_790_140_800_000, 30_923.374)]);
        let v: Value = serde_json::from_str(r#"{"code":"0","data":[["1790140500000","3004981.26","30049.8126","2611331719.92"],["1790140200000","3005435.61","30054.3561","2611849773.38"]]}"#).unwrap();
        assert_eq!(parse_okx_history(&v), vec![(1_790_140_200_000, 30_054.3561), (1_790_140_500_000, 30_049.8126)]);
        let v: Value = serde_json::from_str(r#"{"retCode":0,"result":{"list":[["1790140800000","86836.7","86879.48","86727","86783.06"]]}}"#).unwrap();
        assert_eq!(parse_bybit_marks(&v).get(&1_790_140_800_000), Some(&86_783.06));
    }

    #[test]
    fn an_unknown_symbol_is_unlisted_not_failing() {
        assert_eq!(reading(Ok(Some(5.0))), Reading::Coins(5.0));
        assert_eq!(reading(Ok(None)), Reading::Unlisted);
        assert_eq!(reading(Err("400 Bad Request: {\"code\":-1121}".into())), Reading::Unlisted);
        assert!(matches!(reading(Err("error sending request".into())), Reading::Failed(_)));
    }

    #[test]
    fn venues_are_summed_and_a_quiet_one_carries_forward() {
        let c = |v: f64| Ohlc { open: v, high: v + 1.0, low: v - 1.0, close: v };
        let rows = vec![
            ("a".to_string(), 0, c(10.0)),
            ("b".to_string(), 0, c(20.0)),
            ("a".to_string(), 60_000, c(11.0)),
            // b says nothing at 60 000: its 20 carries.
        ];
        let out = aggregate(&rows, &HashMap::new());
        assert_eq!(out[0], (0, Ohlc { open: 30.0, high: 32.0, low: 28.0, close: 30.0 }));
        assert_eq!(out[1], (60_000, Ohlc { open: 31.0, high: 32.0, low: 30.0, close: 31.0 }));
        // A seed counts from the first bucket; one older than STALE_MS does not.
        let seed = HashMap::from([("c".to_string(), (-60_000, 5.0)), ("d".to_string(), (-STALE_MS - 60_000, 7.0))]);
        assert_eq!(aggregate(&rows, &seed)[0].1.close, 35.0);
    }

    #[test]
    fn a_venue_that_joins_later_is_estimated_before_it_did() {
        let flat = |v: f64| Ohlc { open: v, high: v, low: v, close: v };
        // `a` from the start at 100 then 200; `b` joins in the second bucket at 50 — a quarter of `a` there.
        let rows = vec![("a".to_string(), 0, flat(100.0)), ("a".to_string(), 60_000, flat(200.0)), ("b".to_string(), 60_000, flat(50.0))];
        let out = aggregate(&rows, &HashMap::new());
        assert_eq!(out[0].1.close, 125.0, "no cliff where b starts: it is a quarter of a before");
        assert_eq!(out[1].1.close, 250.0);
    }
}
