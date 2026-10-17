//! Series this server works out itself, under the `index` venue: the crypto
//! market's capitalisation and its parts, as TradingView's CRYPTOCAP names
//! them, and the VIX.
//!
//! CoinGecko's free `/global` gives what every coin it follows is worth
//! together and the share each of the ten largest holds, but moves only about
//! every ten minutes. Its `/coins/markets` moves every minute: the 250 largest
//! coins' worth, read beside it, carries CoinGecko's figures from one update to
//! the next — each part moved as its own coins have since. Kept a row a minute
//! (`market_cap`), it is drawn as candles of any length, from when the
//! recording began, CoinGecko keeping its own history for paying subscribers.
//! The stablecoins' worth, for TOTALES, is read every ten minutes. The VIX is
//! Cboe's: its daily bars since 1990 from the CSV it publishes, read every
//! hour, and its delayed quote every minute — the day's own bar until the
//! history carries it (`index_bar`), and each new value as a point
//! (`index_tick`), from which its bars shorter than a day are drawn.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail};
use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;
use tradoor_store::now_ms;
use tradoor_venues::{Product, Venue};

use crate::api::{ApiError, ApiResult, AppState};
use crate::dns::Doh;

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
/// The epoch began on a Thursday; weeks here begin on Monday, as the exchanges' do.
const MONDAY: i64 = 4 * DAY;

const GLOBAL: &str = "https://api.coingecko.com/api/v3/global";
const COINS: &str = "https://api.coingecko.com/api/v3/coins/markets?vs_currency=usd&order=market_cap_desc&per_page=250&page=1";
const STABLECOINS: &str = "https://api.coingecko.com/api/v3/coins/markets?vs_currency=usd&category=stablecoins&per_page=250&page=1";
const VIX_HISTORY: &str = "https://cdn.cboe.com/api/global/us_indices/daily_prices/VIX_History.csv";
const VIX_QUOTE: &str = "https://cdn.cboe.com/api/global/delayed_quotes/quotes/_VIX.json";

const READ_EVERY: Duration = Duration::from_secs(60);
/// Reads between two of the stablecoins' worth, and of the VIX's history.
const STABLE_EVERY: u32 = 10;
const HISTORY_EVERY: u32 = 60;
/// Bars one request returns at most.
const MAX_BARS: i64 = 2000;
/// Minutes of the coins' worth kept, to pair CoinGecko's next figures with the minute they are of.
const RECENT: usize = 30;

/// One series: its symbol, what it is, and where its numbers come from.
struct Series {
    symbol: &'static str,
    name: &'static str,
    /// What it is counted in, for the search's `base/quote`.
    quote: &'static str,
    decimals: u8,
    source: Source,
}

enum Source {
    /// Worked out from each `market_cap` row: an expression of its columns.
    Cap(&'static str),
    /// Daily bars kept in `index_bar`, and the value through the day in `index_tick`.
    Quoted,
}

/// Capitalisations in billions of dollars, so a chart's scale reads `2,756.79`
/// rather than thirteen digits; shares in percent.
const SERIES: [Series; 10] = [
    Series { symbol: "TOTAL", name: "Crypto total market cap, $B", quote: "USD", decimals: 2, source: Source::Cap("total / 1e9") },
    Series { symbol: "TOTAL2", name: "Crypto market cap less Bitcoin, $B", quote: "USD", decimals: 2, source: Source::Cap("(total - btc) / 1e9") },
    Series { symbol: "TOTAL3", name: "Crypto market cap less Bitcoin and Ether, $B", quote: "USD", decimals: 2, source: Source::Cap("(total - btc - eth) / 1e9") },
    Series { symbol: "TOTALES", name: "Crypto market cap less stablecoins, $B", quote: "USD", decimals: 2, source: Source::Cap("(total - stable) / 1e9") },
    Series { symbol: "OTHERS", name: "Crypto market cap less the ten largest, $B", quote: "USD", decimals: 2, source: Source::Cap("(total - top10) / 1e9") },
    Series { symbol: "OTHERS_D", name: "Share outside the ten largest, %", quote: "%", decimals: 2, source: Source::Cap("(total - top10) / total * 100") },
    Series { symbol: "BTC_D", name: "Bitcoin dominance, %", quote: "%", decimals: 2, source: Source::Cap("btc / total * 100") },
    Series { symbol: "ETH_D", name: "Ether dominance, %", quote: "%", decimals: 2, source: Source::Cap("eth / total * 100") },
    Series { symbol: "USDT_D", name: "Tether dominance, %", quote: "%", decimals: 2, source: Source::Cap("usdt / total * 100") },
    Series { symbol: "VIX", name: "Cboe Volatility Index, delayed", quote: "", decimals: 2, source: Source::Quoted },
];

fn series(symbol: &str) -> Option<&'static Series> {
    SERIES.iter().find(|s| s.symbol == symbol)
}

impl Series {
    /// A value at the series' own decimals: a chart reads its precision off the prices it is given.
    fn round(&self, v: f64) -> f64 {
        let scale = 10f64.powi(i32::from(self.decimals));
        (v * scale).round() / scale
    }
}

pub struct Indices {
    pool: deadpool_postgres::Pool,
    http: reqwest::Client,
    /// The stablecoins' worth as last read, carried into each row until the next.
    stable: Mutex<Option<f64>>,
    caps: Mutex<Caps>,
}

/// CoinGecko's latest figures with the coins' worth then, and the last
/// minutes' worth to pair its next ones with.
#[derive(Default)]
struct Caps {
    anchor: Option<Anchor>,
    recent: VecDeque<(i64, Coins)>,
}

impl Indices {
    pub fn start(pool: deadpool_postgres::Pool, doh: &Arc<Doh>) -> anyhow::Result<Arc<Self>> {
        let http = reqwest::Client::builder().dns_resolver(doh.resolver()).user_agent(concat!("tradoor/", env!("CARGO_PKG_VERSION"))).timeout(Duration::from_secs(30)).build()?;
        let indices = Arc::new(Self { pool, http, stable: Mutex::new(None), caps: Mutex::new(Caps::default()) });
        tokio::spawn(indices.clone().run());
        Ok(indices)
    }

    async fn run(self: Arc<Self>) {
        for read in 0u32.. {
            if read % STABLE_EVERY == 0 {
                if let Err(e) = self.read_stablecoins().await {
                    tracing::warn!(error = %e, "reading the stablecoins' worth");
                }
            }
            if let Err(e) = self.read_market_cap().await {
                tracing::warn!(error = %e, "reading the market cap");
            }
            if read % HISTORY_EVERY == 0 {
                if let Err(e) = self.read_vix_history().await {
                    tracing::warn!(error = %e, "reading the VIX's history");
                }
            }
            if let Err(e) = self.read_vix_quote().await {
                tracing::warn!(error = %e, "reading the VIX");
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    }

    async fn text(&self, url: &str) -> anyhow::Result<String> {
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            bail!("{status} from {url}");
        }
        Ok(body)
    }

    async fn read_stablecoins(&self) -> anyhow::Result<()> {
        let list: Value = serde_json::from_str(&self.text(STABLECOINS).await?)?;
        let worth: f64 = list.as_array().ok_or_else(|| anyhow!("no stablecoin list"))?.iter().filter_map(|c| c["market_cap"].as_f64()).sum();
        if worth > 0.0 {
            *self.stable.lock().await = Some(worth);
        }
        Ok(())
    }

    /// This minute's market cap: CoinGecko's figures, moved since they were
    /// made as the coins have — or as they are, while the coins do not answer.
    async fn read_market_cap(&self) -> anyhow::Result<()> {
        let (global, list) = tokio::join!(self.text(GLOBAL), self.text(COINS));
        let global = global.ok().and_then(|b| serde_json::from_str::<Value>(&b).ok()).and_then(|v| market_cap(&v["data"]));
        let coins = list.ok().and_then(|b| serde_json::from_str::<Value>(&b).ok()).and_then(|v| coins_of(&v));
        let minute = now_ms().div_euclid(MINUTE) * MINUTE;
        let (at, cap) = {
            let mut caps = self.caps.lock().await;
            if let Some(c) = coins {
                caps.recent.push_back((minute, c));
                while caps.recent.len() > RECENT {
                    caps.recent.pop_front();
                }
            }
            if let Some((at, cap)) = global {
                if caps.anchor.is_none_or(|a| a.at != at) {
                    // New figures, paired with the coins' worth nearest their time.
                    if let Some(&(_, then)) = caps.recent.iter().min_by_key(|(m, _)| (m - at).abs()) {
                        caps.anchor = Some(Anchor { at, cap, coins: then });
                    }
                }
            }
            match (caps.anchor, coins) {
                (Some(anchor), Some(c)) => (minute, anchor.now(&c)),
                _ => global.ok_or_else(|| anyhow!("neither of CoinGecko's answers came"))?,
            }
        };
        let stable = *self.stable.lock().await;
        let client = self.pool.get().await?;
        client
            .execute(
                "INSERT INTO market_cap (at, total, btc, eth, usdt, top10, stable) VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (at) DO UPDATE SET total = EXCLUDED.total, btc = EXCLUDED.btc, eth = EXCLUDED.eth, usdt = EXCLUDED.usdt, top10 = EXCLUDED.top10, stable = EXCLUDED.stable",
                &[&at, &cap.total, &cap.btc, &cap.eth, &cap.usdt, &cap.top10, &stable],
            )
            .await?;
        Ok(())
    }

    async fn read_vix_history(&self) -> anyhow::Result<()> {
        let bars = cboe_history(&self.text(VIX_HISTORY).await?);
        if bars.is_empty() {
            bail!("no bars in Cboe's VIX history");
        }
        let client = self.pool.get().await?;
        // The whole history the first time; after that the last week, which a correction may touch.
        let latest: Option<i64> = client.query_one("SELECT max(open_time) FROM index_bar WHERE symbol = 'VIX' AND final", &[]).await?.get(0);
        let from = latest.map_or(i64::MIN, |t| t - 7 * DAY);
        let fresh: Vec<&Bar> = bars.iter().filter(|b| b.open_time >= from).collect();
        let col = |f: fn(&Bar) -> f64| fresh.iter().map(|b| f(b)).collect::<Vec<f64>>();
        client
            .execute(
                "INSERT INTO index_bar (symbol, open_time, open, high, low, close, final)
                 SELECT 'VIX', *, true FROM UNNEST($1::bigint[], $2::float8[], $3::float8[], $4::float8[], $5::float8[])
                 ON CONFLICT (symbol, open_time) DO UPDATE SET open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, close = EXCLUDED.close, final = true",
                &[&fresh.iter().map(|b| b.open_time).collect::<Vec<i64>>(), &col(|b| b.open), &col(|b| b.high), &col(|b| b.low), &col(|b| b.close)],
            )
            .await?;
        Ok(())
    }

    /// The delayed quote: a point at its own time, and the day's bar until
    /// Cboe's history carries the day.
    async fn read_vix_quote(&self) -> anyhow::Result<()> {
        let v: Value = serde_json::from_str(&self.text(VIX_QUOTE).await?)?;
        let bar = cboe_quote(&v["data"]).ok_or_else(|| anyhow!("no VIX quote in Cboe's answer"))?;
        let client = self.pool.get().await?;
        // A quote unchanged since — a weekend's, a night's — is the same point again.
        if let Some(at) = v["data"]["last_trade_time"].as_str().and_then(eastern_time) {
            client.execute("INSERT INTO index_tick (symbol, at, value) VALUES ('VIX', $1, $2) ON CONFLICT DO NOTHING", &[&at, &bar.close]).await?;
        }
        client
            .execute(
                "INSERT INTO index_bar (symbol, open_time, open, high, low, close, final) VALUES ('VIX', $1, $2, $3, $4, $5, false)
                 ON CONFLICT (symbol, open_time) DO UPDATE SET open = EXCLUDED.open, high = EXCLUDED.high, low = EXCLUDED.low, close = EXCLUDED.close
                 WHERE NOT index_bar.final",
                &[&bar.open_time, &bar.open, &bar.high, &bar.low, &bar.close],
            )
            .await?;
        Ok(())
    }

    /// The series as the symbol search lists them, each with its latest value
    /// and its change over a day.
    pub async fn products(&self) -> Vec<Product> {
        let Ok(client) = self.pool.get().await else { return Vec::new() };
        let mut out = Vec::new();
        for s in &SERIES {
            let quote = match s.source {
                Source::Cap(expr) => {
                    let sql = format!(
                        "SELECT (SELECT {expr} FROM market_cap ORDER BY at DESC LIMIT 1), (SELECT {expr} FROM market_cap WHERE at <= $1 ORDER BY at DESC LIMIT 1)"
                    );
                    client.query_one(&sql, &[&(now_ms() - DAY)]).await.ok().map(|r| (r.get::<_, Option<f64>>(0), r.get::<_, Option<f64>>(1)))
                }
                Source::Quoted => client
                    .query_one(
                        "SELECT (array_agg(close ORDER BY open_time DESC))[1], (array_agg(close ORDER BY open_time DESC))[2] FROM index_bar WHERE symbol = $1",
                        &[&s.symbol],
                    )
                    .await
                    .ok()
                    .map(|r| (r.get::<_, Option<f64>>(0), r.get::<_, Option<f64>>(1))),
            };
            let (last, before) = quote.unwrap_or((None, None));
            let last = last.map(|v| s.round(v));
            out.push(Product {
                market: format!("{}:{}", Venue::Index.id(), s.symbol),
                venue: Venue::Index.id(),
                symbol: s.symbol.to_string(),
                base: s.symbol.to_string(),
                quote: s.quote.to_string(),
                kind: "index",
                native: None,
                name: Some(s.name.to_string()),
                price_decimals: Some(s.decimals),
                quote_volume_24h: None,
                last_price: last,
                change_pct: last.zip(before).filter(|(_, b)| *b != 0.0).map(|(l, b)| (l / b - 1.0) * 100.0),
            });
        }
        out
    }
}

/// What CoinGecko's `/global` says, in dollars, and when it says it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cap {
    total: f64,
    btc: f64,
    eth: f64,
    usdt: f64,
    top10: f64,
}

fn market_cap(data: &Value) -> Option<(i64, Cap)> {
    let total = data["total_market_cap"]["usd"].as_f64().filter(|t| *t > 0.0)?;
    let shares = data["market_cap_percentage"].as_object()?;
    let part = |coin: &str| shares.get(coin).and_then(Value::as_f64).unwrap_or(0.0) * total / 100.0;
    let mut largest: Vec<f64> = shares.values().filter_map(Value::as_f64).collect();
    largest.sort_by(|a, b| b.total_cmp(a));
    let top10 = largest.iter().take(10).sum::<f64>() * total / 100.0;
    let at = data["updated_at"].as_i64().map_or_else(now_ms, |s| s * 1000);
    Some((at, Cap { total, btc: part("btc"), eth: part("eth"), usdt: part("usdt"), top10 }))
}

/// What `/coins/markets` says the 250 largest coins are worth, by the parts
/// CoinGecko's figures are made of.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Coins {
    all: f64,
    btc: f64,
    eth: f64,
    usdt: f64,
    top10: f64,
}

fn coins_of(list: &Value) -> Option<Coins> {
    let list = list.as_array()?;
    let cap = |c: &Value| c["market_cap"].as_f64().filter(|v| *v > 0.0);
    let of = |id: &str| list.iter().find(|c| c["id"] == id).and_then(cap).unwrap_or(0.0);
    let mut caps: Vec<f64> = list.iter().filter_map(cap).collect();
    caps.sort_by(|a, b| b.total_cmp(a));
    let all: f64 = caps.iter().sum();
    (all > 0.0).then(|| Coins { all, btc: of("bitcoin"), eth: of("ethereum"), usdt: of("tether"), top10: caps.iter().take(10).sum() })
}

/// CoinGecko's figures as of `at`, and the coins' worth then.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    at: i64,
    cap: Cap,
    coins: Coins,
}

impl Anchor {
    /// The figures now: each part moved as its coins have since — the whole
    /// as the 250 have, Bitcoin as Bitcoin has.
    fn now(&self, c: &Coins) -> Cap {
        let moved = |figure: f64, then: f64, now: f64| if then > 0.0 && now > 0.0 { figure * now / then } else { figure };
        Cap {
            total: moved(self.cap.total, self.coins.all, c.all),
            btc: moved(self.cap.btc, self.coins.btc, c.btc),
            eth: moved(self.cap.eth, self.coins.eth, c.eth),
            usdt: moved(self.cap.usdt, self.coins.usdt, c.usdt),
            top10: moved(self.cap.top10, self.coins.top10, c.top10),
        }
    }
}

#[derive(Debug, PartialEq)]
struct Bar {
    open_time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

/// Days since the epoch of a date in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// New York's wall-clock `YYYY-MM-DDTHH:MM:SS` as a moment: Eastern Daylight
/// Time from 2 a.m. on the second Sunday of March to 2 a.m. on the first
/// Sunday of November, Eastern Standard Time otherwise.
fn eastern_time(text: &str) -> Option<i64> {
    let (date, time) = text.get(..19)?.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
    let local = days_from_civil(year, month, day) * DAY + t.next()?? * HOUR + t.next()?? * MINUTE + t.next()?? * 1000;
    let sunday_from = |day: i64| day + (7 - (day + 4).rem_euclid(7)) % 7;
    let summer = (sunday_from(days_from_civil(year, 3, 1)) + 7) * DAY + 2 * HOUR..sunday_from(days_from_civil(year, 11, 1)) * DAY + 2 * HOUR;
    Some(local + if summer.contains(&local) { 4 * HOUR } else { 5 * HOUR })
}

/// `DATE,OPEN,HIGH,LOW,CLOSE` with dates as `MM/DD/YYYY`, oldest first.
fn cboe_history(text: &str) -> Vec<Bar> {
    text.lines()
        .filter_map(|line| {
            let mut cols = line.trim().split(',');
            let mut date = cols.next()?.split('/').map(|p| p.parse::<i64>().ok());
            let (month, day, year) = (date.next()??, date.next()??, date.next()??);
            let mut num = || cols.next()?.parse::<f64>().ok().filter(|v| *v > 0.0);
            Some(Bar { open_time: days_from_civil(year, month, day) * DAY, open: num()?, high: num()?, low: num()?, close: num()? })
        })
        .collect()
}

/// The day's bar from Cboe's delayed quote: its day is the last trade's, as
/// Chicago and New York date it.
fn cboe_quote(data: &Value) -> Option<Bar> {
    let price = data["current_price"].as_f64().filter(|p| *p > 0.0)?;
    let day = data["last_trade_time"].as_str()?.get(..10)?;
    let mut parts = day.split('-').map(|p| p.parse::<i64>().ok());
    let (year, month, date) = (parts.next()??, parts.next()??, parts.next()??);
    let at = |key: &str| data[key].as_f64().filter(|v| *v > 0.0);
    let open = at("open").unwrap_or(price);
    Some(Bar {
        open_time: days_from_civil(year, month, date) * DAY,
        open,
        high: at("high").unwrap_or(price).max(price).max(open),
        low: at("low").unwrap_or(price).min(price).min(open),
        close: price,
    })
}

/// A bar length: so many milliseconds, from an offset; or the calendar month.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Fixed(i64, i64),
    Month,
}

fn step_of(interval: &str) -> Option<Step> {
    let ms = match interval {
        "1m" => MINUTE,
        "5m" => 5 * MINUTE,
        "15m" => 15 * MINUTE,
        "30m" => 30 * MINUTE,
        "1h" => 60 * MINUTE,
        "2h" => 120 * MINUTE,
        "4h" => 240 * MINUTE,
        "6h" => 360 * MINUTE,
        "12h" => 720 * MINUTE,
        "1d" => DAY,
        "1w" => return Some(Step::Fixed(7 * DAY, MONDAY)),
        "1M" => return Some(Step::Month),
        _ => return None,
    };
    Some(Step::Fixed(ms, 0))
}

impl Step {
    /// Roughly how long a bar is, for the span a number of them takes.
    fn span(self) -> i64 {
        match self {
            Step::Fixed(ms, _) => ms,
            Step::Month => 31 * DAY,
        }
    }

    /// SQL for the opening of the bar a moment `at` (in milliseconds) falls in.
    fn opening(self, at: &str) -> String {
        match self {
            Step::Fixed(ms, offset) => format!("(({at} - {offset}) / {ms}) * {ms} + {offset}"),
            Step::Month => format!("(extract(epoch FROM date_trunc('month', to_timestamp({at} / 1000.0) AT TIME ZONE 'UTC')) * 1000)::bigint"),
        }
    }
}

#[derive(Deserialize)]
pub struct Ask {
    symbol: String,
    interval: String,
    start: Option<i64>,
    end: Option<i64>,
    limit: Option<i64>,
}

/// `GET /api/index/candles?symbol=TOTAL2&interval=1h&end=&limit=`: an index's
/// bars as `[open time, open, high, low, close, volume]`, oldest first, the
/// forming one with them — from `start` on when it is given, else the newest
/// up to `end`, as the exchanges answer.
pub async fn candles(State(state): State<AppState>, Query(ask): Query<Ask>) -> ApiResult<Json<Value>> {
    let s = series(&ask.symbol).ok_or_else(|| ApiError::bad_request("no such index"))?;
    let step = step_of(&ask.interval).ok_or_else(|| ApiError::bad_request("no such bar length"))?;
    let limit = ask.limit.unwrap_or(500).clamp(1, MAX_BARS);
    // The moments the bars asked for can hold: a bar more either side, cut to `limit` below.
    let (from, to) = match (ask.start, ask.end) {
        (Some(start), end) => (start, end.unwrap_or(i64::MAX / 2).min(start + (limit + 1) * step.span())),
        (None, end) => {
            let end = end.unwrap_or_else(now_ms);
            (end - (limit + 1) * step.span(), end + step.span())
        }
    };
    let intraday = matches!(step, Step::Fixed(ms, _) if ms < DAY);
    // Bars of points in time — each its first and last value and its extremes —
    // or of a day's bars, which carry their own.
    let points = |table: &str, value: &str, symbol: &str| {
        format!(
            "SELECT b, (array_agg(v ORDER BY at))[1], max(v), min(v), (array_agg(v ORDER BY at DESC))[1]
             FROM (SELECT {} AS b, at, {value} AS v FROM {table} WHERE at >= $1 AND at < $2 {symbol}) s
             WHERE v IS NOT NULL GROUP BY b ORDER BY b",
            step.opening("at")
        )
    };
    let client = state.pool.get().await?;
    let rows = match s.source {
        Source::Cap(expr) => client.query(&points("market_cap", expr, ""), &[&from, &to]).await?,
        Source::Quoted if intraday => client.query(&points("index_tick", "value", "AND symbol = $3"), &[&from, &to, &s.symbol]).await?,
        Source::Quoted => {
            let sql = format!(
                "SELECT {} AS b, (array_agg(open ORDER BY open_time))[1], max(high), min(low), (array_agg(close ORDER BY open_time DESC))[1]
                 FROM index_bar WHERE symbol = $3 AND open_time >= $1 AND open_time < $2 GROUP BY b ORDER BY b",
                step.opening("open_time")
            );
            client.query(&sql, &[&from, &to, &s.symbol]).await?
        }
    };
    let mut bars: Vec<Value> = rows
        .iter()
        .filter(|r| ask.start.is_none_or(|start| r.get::<_, i64>(0) >= start) && ask.end.is_none_or(|end| r.get::<_, i64>(0) <= end))
        .map(|r| Value::from(vec![r.get::<_, i64>(0) as f64, s.round(r.get(1)), s.round(r.get(2)), s.round(r.get(3)), s.round(r.get(4)), 0.0]))
        .collect();
    let keep = limit as usize;
    if ask.start.is_some() {
        bars.truncate(keep);
    } else if bars.len() > keep {
        bars.drain(..bars.len() - keep);
    }
    Ok(Json(Value::Array(bars)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_count_from_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2026, 10, 16) * DAY, 1_792_108_800_000);
        assert_eq!((days_from_civil(2026, 10, 12) * DAY - MONDAY) % (7 * DAY), 0, "a Monday opens a week");
    }

    #[test]
    fn cboe_s_history_reads_as_daily_bars() {
        let text = "DATE,OPEN,HIGH,LOW,CLOSE\n01/02/1990,17.240000,17.240000,17.240000,17.240000\n10/16/2026,16.020000,22.310000,15.720000,19.990000\n";
        let bars = cboe_history(text);
        assert_eq!(bars.len(), 2, "the header is no bar");
        assert_eq!(bars[1], Bar { open_time: 1_792_108_800_000, open: 16.02, high: 22.31, low: 15.72, close: 19.99 });
    }

    #[test]
    fn the_quote_is_the_day_of_its_last_trade() {
        let data: Value = serde_json::from_str(r#"{"current_price": 19.99, "open": 15.81, "high": 22.31, "low": 15.72, "last_trade_time": "2026-10-16T16:15:01"}"#).unwrap();
        assert_eq!(cboe_quote(&data), Some(Bar { open_time: 1_792_108_800_000, open: 15.81, high: 22.31, low: 15.72, close: 19.99 }));
        let early: Value = serde_json::from_str(r#"{"current_price": 18.5, "open": 0, "high": 0, "low": 0, "last_trade_time": "2026-10-19T03:20:00"}"#).unwrap();
        let bar = cboe_quote(&early).unwrap();
        assert_eq!((bar.open, bar.high, bar.low), (18.5, 18.5, 18.5), "before the open, the price is all of the bar");
    }

    #[test]
    fn coingecko_s_shares_become_dollars() {
        let data: Value = serde_json::from_str(
            r#"{"total_market_cap": {"usd": 2000.0}, "updated_at": 1792235000,
                "market_cap_percentage": {"btc": 60.0, "eth": 10.0, "usdt": 5.0, "bnb": 3.0, "xrp": 2.0, "usdc": 2.0, "sol": 2.0, "trx": 1.0, "doge": 1.0, "steth": 1.0}}"#,
        )
        .unwrap();
        let (at, cap) = market_cap(&data).unwrap();
        assert_eq!(at, 1_792_235_000_000);
        assert_eq!(cap, Cap { total: 2000.0, btc: 1200.0, eth: 200.0, usdt: 100.0, top10: 1740.0 });
    }

    #[test]
    fn the_coins_carry_coingecko_s_figures_between_its_updates() {
        let list: Value = serde_json::from_str(
            r#"[{"id": "bitcoin", "market_cap": 1600.0}, {"id": "ethereum", "market_cap": 300.0}, {"id": "tether", "market_cap": 100.0}, {"id": "dust", "market_cap": null}]"#,
        )
        .unwrap();
        let then = coins_of(&list).unwrap();
        assert_eq!(then, Coins { all: 2000.0, btc: 1600.0, eth: 300.0, usdt: 100.0, top10: 2000.0 });
        let anchor = Anchor { at: 0, cap: Cap { total: 2100.0, btc: 1590.0, eth: 310.0, usdt: 100.0, top10: 2050.0 }, coins: then };
        // Bitcoin up 10%, the rest still.
        let now = Coins { all: 2160.0, btc: 1760.0, ..then };
        let cap = anchor.now(&now);
        assert!((cap.total - 2268.0).abs() < 1e-9 && (cap.btc - 1749.0).abs() < 1e-9, "{cap:?}");
        assert_eq!((cap.eth, cap.usdt), (310.0, 100.0), "the parts that did not move stay as CoinGecko had them");
    }

    #[test]
    fn new_york_s_clock_reads_as_utc() {
        assert_eq!(eastern_time("2026-10-16T16:15:01"), Some(days_from_civil(2026, 10, 16) * DAY + 20 * HOUR + 15 * MINUTE + 1000), "October: daylight time, four hours behind");
        assert_eq!(eastern_time("2026-12-01T09:30:00"), Some(days_from_civil(2026, 12, 1) * DAY + 14 * HOUR + 30 * MINUTE), "December: standard time, five behind");
        assert_eq!(eastern_time("2026-03-08T01:59:00"), Some(days_from_civil(2026, 3, 8) * DAY + 6 * HOUR + 59 * MINUTE), "just before the clocks go forward");
        assert_eq!(eastern_time("2026-03-08T03:00:00"), Some(days_from_civil(2026, 3, 8) * DAY + 7 * HOUR), "and after");
        assert_eq!(eastern_time("2026-11-01T02:30:00"), Some(days_from_civil(2026, 11, 1) * DAY + 7 * HOUR + 30 * MINUTE), "back on standard time");
        assert_eq!(eastern_time("yesterday"), None);
    }

    #[test]
    fn bars_open_on_their_boundaries() {
        assert_eq!(step_of("4h"), Some(Step::Fixed(4 * 3_600_000, 0)));
        assert_eq!(step_of("1w"), Some(Step::Fixed(7 * DAY, MONDAY)));
        assert_eq!(step_of("3d"), None, "folded by the page from days");
        assert_eq!(Step::Fixed(7 * DAY, MONDAY).opening("at"), format!("((at - {MONDAY}) / {}) * {} + {MONDAY}", 7 * DAY, 7 * DAY));
        assert!(SERIES.iter().all(|s| tradoor_venues::MarketRef::new(Venue::Index, s.symbol).is_some()), "every symbol is a market's");
        assert_eq!(series("BTC_D").unwrap().round(59.30406459), 59.3);
    }
}
