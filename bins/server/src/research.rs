//! `tradoor-server signal-research out.jsonl`
//!
//! The screener's four-hour rules (`drives`) replayed over a long history of
//! the coins it screens — some five hundred days where a coin has them, not
//! the replay's 166 — with each pattern written out as one JSON line: what a
//! scan knew of it when it was first seen and when it turned, what else was
//! known then (the trend it came out of, its volatility, Bitcoin's), and the
//! bars that followed, so that filters and trade plans can be weighed offline
//! without running the screener again. Reads the exchanges' public klines
//! only; needs no database.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tradoor_core::indicators::{ema, rsi, stoch_rsi};
use tradoor_core::{Candle, Interval};
use tradoor_store::now_ms;
use tradoor_venues::{binance, bybit, Exchange, MarketRef};

use crate::catalog::Catalog;
use crate::dns::Doh;
use crate::drives::{self, Fill, Found, Plan, Side};
use crate::signals::candles_of;

const STEP: i64 = 4 * 3_600_000;
/// Bars written after the third drive: its wait for the turn, the trade's
/// four days, and as long again for plans that hold longer.
const PATH_BARS: usize = 96;
/// The market Bitcoin's own trend is read from, beside every coin.
const BTC: &str = "binance-perp:BTCUSDT";

pub async fn run(binance_spot_rest: String, out: &Path, coins: usize, bars: usize) -> anyhow::Result<()> {
    let doh = Doh::new()?;
    let http = reqwest::Client::builder().dns_resolver(doh.resolver()).timeout(Duration::from_secs(30)).build()?;
    let catalog = Arc::new(Catalog::new(binance_spot_rest, &doh)?);
    let markets = catalog.perp_leaders(coins).await;
    if markets.is_empty() {
        bail!("the catalogue lists no perpetuals");
    }
    let until = now_ms().div_euclid(STEP) * STEP;
    let btc = history(&http, BTC, until, bars).await.context("Bitcoin's bars")?;
    let btc = Btc::new(&btc.0);
    tracing::info!(markets = markets.len(), bars, "reading the history");
    let read: Vec<(usize, String, anyhow::Result<(Vec<Candle>, Option<Vec<f64>>)>)> = futures_util::stream::iter(markets.into_iter().enumerate())
        .map(|(rank, market)| {
            let http = &http;
            async move {
                let bars = history(http, &market, until, bars).await;
                (rank, market, bars)
            }
        })
        .buffer_unordered(2)
        .collect()
        .await;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?);
    let (mut written, mut failed) = (0, 0);
    for (rank, market, bars) in read {
        let (bars, taker) = match bars {
            Ok(b) => b,
            Err(e) => {
                failed += 1;
                tracing::warn!(%market, error = %e, "bars not read");
                continue;
            }
        };
        for side in [Side::Bear, Side::Bull] {
            for line in patterns(&market, rank, &bars, taker.as_deref(), side, &btc) {
                serde_json::to_writer(&mut file, &line)?;
                file.write_all(b"\n")?;
                written += 1;
            }
        }
    }
    file.flush()?;
    tracing::info!(patterns = written, failed, out = %out.display(), "signal research written");
    Ok(())
}

/// A market's last `count` closed four-hour bars before `until`, oldest first,
/// read back a page at a time, with the taker buy volume where reported.
async fn history(http: &reqwest::Client, market: &str, until: i64, count: usize) -> anyhow::Result<(Vec<Candle>, Option<Vec<f64>>)> {
    let m = MarketRef::parse(market).ok_or_else(|| anyhow!("{market} is not a market"))?;
    let exchange = m.venue.exchange();
    let page: u32 = if exchange == Exchange::Binance { 1500 } else { 1000 };
    let mut klines = Vec::new();
    let mut end = until - 1;
    while klines.len() < count {
        let url = match exchange {
            Exchange::Binance => binance::klines_url(binance::PERP_REST, &m, Interval::H4, None, Some(end), page),
            Exchange::Bybit => bybit::klines_url(bybit::REST, &m, Interval::H4, None, Some(end), page),
            _ => bail!("only Binance and Bybit are screened"),
        };
        let body = get(http, &url).await?;
        let mut got = if exchange == Exchange::Binance { binance::parse_klines(&body)? } else { bybit::parse_klines(&body)? };
        got.retain(|k| k.open_time + STEP <= until);
        let Some(first) = got.first().map(|k| k.open_time) else { break };
        let full = got.len() as u32 >= page - 1;
        got.append(&mut klines);
        klines = got;
        if !full {
            break;
        }
        end = first - 1;
    }
    klines.dedup_by_key(|k| k.open_time);
    let skip = klines.len().saturating_sub(count);
    candles_of(&klines[skip..])
}

/// A page, waiting out the exchange's rate limit when it says so: Binance's
/// is counted by the minute.
async fn get(http: &reqwest::Client, url: &str) -> anyhow::Result<String> {
    for wait in [0, 10, 30, 65] {
        tokio::time::sleep(Duration::from_secs(wait)).await;
        let resp = http.get(url).send().await?;
        let status = resp.status();
        if status.as_u16() == 429 || status.as_u16() == 418 || status.is_server_error() {
            continue;
        }
        let body = resp.text().await?;
        if !status.is_success() {
            bail!("{status} from {url}");
        }
        return Ok(body);
    }
    bail!("{url} kept refusing")
}

/// Bitcoin's trend by bar: how it stood at any coin's bar of the same time.
struct Btc {
    at: HashMap<i64, usize>,
    close: Vec<f64>,
    ema200: Vec<Option<f64>>,
}

impl Btc {
    fn new(bars: &[Candle]) -> Self {
        let closes: Vec<Option<f64>> = bars.iter().map(|k| Some(k.close)).collect();
        Self { at: bars.iter().enumerate().map(|(i, k)| (k.open_time, i)).collect(), close: bars.iter().map(|k| k.close).collect(), ema200: ema(&closes, 200) }
    }

    /// Bitcoin at the bar opening at `at`: its change over a week and a month
    /// before, in percent, and how far it stood over its 200-bar EMA.
    fn context(&self, at: i64) -> Value {
        let Some(&i) = self.at.get(&at) else { return Value::Null };
        let change = |n: usize| i.checked_sub(n).map(|j| pct(self.close[i], self.close[j]));
        json!({
            "ret7d": change(42),
            "ret30d": change(180),
            "overEma200": self.ema200[i].map(|e| pct(self.close[i], e)),
        })
    }
}

fn pct(now: f64, then: f64) -> f64 {
    (now / then - 1.0) * 100.0
}

/// Every pattern on `side` in the bars, one line each.
fn patterns(market: &str, rank: usize, bars: &[Candle], taker: Option<&[f64]>, side: Side, btc: &Btc) -> Vec<Value> {
    let closes: Vec<Option<f64>> = bars.iter().map(|k| Some(k.close)).collect();
    let (rsi, ema50, ema200, stoch) = (rsi(&closes, 14), ema(&closes, 50), ema(&closes, 200), stoch_rsi(&closes, 14, 14, 3, 3));
    // Where a scan first saw each one turned, by its third drive.
    let turned: HashMap<usize, (usize, Found)> = drives::history(bars, taker, side).into_iter().map(|(at, f)| (f.drives[2].index, (at, f))).collect();
    let dir = if side == Side::Bear { -1.0 } else { 1.0 };
    let mut out = Vec::new();
    for (seen, first) in drives::sightings(bars, taker, side) {
        let z = first.drives[2].index;
        let [a, b] = [first.drives[0].index, first.drives[1].index];
        // The pattern as it was when it turned, if it did; else as first seen.
        let (entry_at, f) = match turned.get(&z) {
            Some((at, f)) => (Some(*at), f),
            None => (None, &first),
        };
        // What the scan entering it knew: the bar it read up to.
        let now = entry_at.unwrap_or(seen);
        let close = bars[now].close;
        let at = |series: &[Option<f64>], i: usize| series.get(i).copied().flatten();
        let change = |n: usize| z.checked_sub(n).map(|j| pct(bars[z].close, bars[j].close));
        let volume = |i: usize| bars[i.saturating_sub(2)..=(i + 2).min(now)].iter().map(|k| k.volume).sum::<f64>();
        let features = json!({
            "rsi": [at(&rsi, a), at(&rsi, b), at(&rsi, z), at(&rsi, now)],
            "stochK": [at(&stoch.k, z), at(&stoch.k, now)],
            "stochD": at(&stoch.d, now),
            "atrPct": f.atr / f.extreme.abs() * 100.0,
            "legPct": (f.extreme - f.pullback).abs() / f.extreme.abs() * 100.0,
            "gaps": [b - a, z - b],
            "waitBars": entry_at.map(|t| t - z),
            "fromExtremeAtr": (close - f.extreme).abs() / f.atr,
            "lineOffAtr": line_off(f, side),
            "volumeFade": volume(z) / volume(b).max(f64::MIN_POSITIVE),
            "ret7d": change(42),
            "ret30d": change(180),
            "ret90d": change(540),
            "overEma50": at(&ema50, now).map(|e| pct(close, e)),
            "overEma200": at(&ema200, now).map(|e| pct(close, e)),
            "ema200Slope": at(&ema200, now).zip(now.checked_sub(30).and_then(|j| at(&ema200, j))).map(|(e, then)| pct(e, then)),
            "btc": btc.context(bars[now].open_time),
        });
        // How the trade at the turn went under each plan, as the replay weighs it.
        let plans: Option<Value> = entry_at.map(|t| {
            Plan::ALL
                .iter()
                .map(|plan| {
                    let lv = drives::plan_levels(*plan, side, close, f.extreme, f.pullback, f.atr);
                    let o = drives::play(side, close, lv, &bars[t + 1..], false);
                    (plan.id().to_string(), json!({ "result": o.result.id(), "ret": o.ret, "bars": o.bars, "best": o.best, "worst": o.worst }))
                })
                .collect::<serde_json::Map<String, Value>>()
                .into()
        });
        // The limit the first scan to see it set, and whether price came back to it.
        let tp1 = drives::levels(side, first.extreme, first.extreme, first.pullback, first.atr).tp[0];
        let limit = drives::limit_entry(bars, side, z, first.extreme, tp1, seen).map(|(price, fill)| match fill {
            Fill::At { bar, price: filled, within } => {
                let o = drives::play(side, filled, drives::levels(side, filled, first.extreme, first.pullback, first.atr), &bars[bar..], within);
                json!({ "price": price, "fill": "at", "bar": bar - z, "entry": filled, "within": within, "result": o.result.id(), "ret": o.ret })
            }
            Fill::Missed => json!({ "price": price, "fill": "missed" }),
            Fill::Waiting => json!({ "price": price, "fill": "waiting" }),
        });
        let path: Vec<[f64; 4]> = bars[z + 1..].iter().take(PATH_BARS).map(|k| [k.open, k.high, k.low, k.close]).collect();
        out.push(json!({
            "market": market,
            "rank": rank,
            "side": side.id(),
            "drives": f.drives.map(|d| json!({ "at": d.at, "price": d.price })),
            "seenAt": bars[seen].open_time + STEP,
            "seen": seen - z,
            "seenScore": first.score,
            "entryAt": entry_at.map(|t| bars[t].open_time + STEP),
            "entryBar": entry_at.map(|t| t - z),
            "entry": entry_at.map(|_| close),
            "score": f.score,
            "divergence": f.divergence,
            "deltaDivergence": f.delta_divergence,
            "deltaAgainst": f.delta_against,
            "pushes": f.pushes.map(|p| p.map(|p| p.share)),
            "extreme": f.extreme,
            "pullback": f.pullback,
            "atr": f.atr,
            "features": features,
            "plans": plans,
            "limit": limit,
            "complete": z + PATH_BARS < bars.len(),
            // The bars after the third drive, [open, high, low, close].
            "path": path,
            "dir": dir,
        }));
    }
    out
}

/// How far the third drive sits from the line through the first two, in ATRs,
/// past it positive.
fn line_off(f: &Found, side: Side) -> f64 {
    let [a, b, z] = f.drives;
    let line = a.price + (b.price - a.price) * (z.index - a.index) as f64 / (b.index - a.index) as f64;
    let sign = if side == Side::Bear { 1.0 } else { -1.0 };
    sign * (z.price - line) / f.atr
}
