//! tradoor-ingest — keeping candle history on disk.
//!
//! Minutes are the source of truth. They come from Binance's published archives
//! where those exist (one checksummed zip per month instead of dozens of REST
//! calls) and from REST for everything else, and only CLOSED bars are ever
//! written. Hours and days are rolled up from the stored minutes, never fetched
//! separately, so the three bases cannot disagree with each other.
#![forbid(unsafe_code)]

use std::io::Read;
use std::time::Duration;

use tradoor_codec::{Period, TickCandles, Tier};
use tradoor_core::time::DAY;
use tradoor_core::Interval;
use tradoor_store::{DataDir, Db};
use tradoor_venues::kline::to_ticks;
use tradoor_venues::{binance, bybit, Exchange, MarketRef, OwnedKline};

const MINUTE: i64 = 60_000;

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    #[error("{0}")]
    Http(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Codec(#[from] tradoor_codec::CodecError),
    #[error(transparent)]
    Venue(#[from] tradoor_venues::VenueError),
    #[error(transparent)]
    Store(#[from] tradoor_store::StoreError),
    #[error("checksum mismatch for {0}")]
    Checksum(String),
    #[error("archive: {0}")]
    Archive(String),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub binance_spot_rest: String,
    pub binance_perp_rest: String,
    pub bybit_rest: String,
    pub use_archives: bool,
    /// Pause between REST calls, to stay well inside public rate limits.
    pub pace: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            binance_spot_rest: binance::SPOT_REST.to_string(),
            binance_perp_rest: binance::PERP_REST.to_string(),
            bybit_rest: bybit::REST.to_string(),
            use_archives: true,
            pace: Duration::from_millis(120),
        }
    }
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct SyncStats {
    pub archives: u32,
    pub rest_requests: u32,
    pub bars_fetched: u64,
    pub files_written: u32,
    pub gaps: u32,
}

pub struct Ingest {
    http: reqwest::Client,
    data: DataDir,
    config: Config,
}

fn day_periods(from: i64, to: i64) -> impl Iterator<Item = Period> {
    let first = Interval::D1.align(from);
    (0..).map(move |i| first + i * DAY).take_while(move |&t| t < to).map(|t| Period::containing(t, Tier::Day))
}

fn next_month(p: Period) -> Period {
    Period::containing(p.end(), Tier::Month)
}

/// Ranges of whole minutes in `[lo, hi)` that `have` has no bar for.
fn missing_ranges(have: &TickCandles, lo: i64, hi: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    let mut cursor = lo;
    for &t in have.open_time.iter().filter(|&&t| t >= lo && t < hi) {
        if t > cursor {
            out.push((cursor, t));
        }
        cursor = t + MINUTE;
    }
    if cursor < hi {
        out.push((cursor, hi));
    }
    out
}

/// Gaps BETWEEN bars held. Before the first bar a market may simply not have
/// been listed yet, which is not a hole in anything.
fn interior_gaps(have: &TickCandles) -> Vec<(i64, i64)> {
    have.open_time.windows(2).filter(|w| w[1] - w[0] > MINUTE).map(|w| (w[0] + MINUTE, w[1])).collect()
}

fn merged(parts: &[TickCandles]) -> Result<TickCandles, IngestError> {
    Ok(TickCandles::merge(parts)?.unwrap_or_else(|| TickCandles::empty(Interval::M1)))
}

fn dedupe(klines: &mut Vec<OwnedKline>) {
    klines.sort_by_key(|k| k.open_time);
    klines.dedup_by_key(|k| k.open_time);
}

impl Ingest {
    pub fn new(data: DataDir, config: Config) -> Result<Self, IngestError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("tradoor/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| IngestError::Http(e.to_string()))?;
        Ok(Self { http, data, config })
    }

    pub fn data(&self) -> &DataDir {
        &self.data
    }

    /// GET with retries on rate limits, server errors and dropped connections.
    /// `None` means 404 — for an archive, "not published".
    async fn get(&self, url: &str) -> Result<Option<Vec<u8>>, IngestError> {
        let mut delay = Duration::from_secs(1);
        for attempt in 0..6 {
            match self.http.get(url).send().await {
                Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => return Ok(None),
                Ok(resp) if resp.status().is_success() => {
                    return resp.bytes().await.map(|b| Some(b.to_vec())).map_err(|e| IngestError::Http(format!("{url}: {e}")));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retryable = matches!(status.as_u16(), 418 | 429) || status.is_server_error();
                    if let Some(secs) = resp.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()?.parse::<u64>().ok()) {
                        delay = delay.max(Duration::from_secs(secs));
                    }
                    if !retryable || attempt == 5 {
                        let body: String = resp.text().await.unwrap_or_default().chars().take(300).collect();
                        return Err(IngestError::Http(format!("{status} from {url}: {body}")));
                    }
                }
                Err(e) if attempt == 5 => return Err(IngestError::Http(format!("{url}: {e}"))),
                Err(e) => tracing::warn!(url, error = %e, "request failed"),
            }
            tracing::warn!(url, attempt, ?delay, "retrying");
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(60));
        }
        unreachable!("the last attempt returns")
    }

    /// One month or day of 1m klines from data.binance.vision, checksum-verified.
    async fn archive(&self, market: &MarketRef, period: Period, stats: &mut SyncStats) -> Result<Option<TickCandles>, IngestError> {
        if !self.config.use_archives {
            return Ok(None);
        }
        let Some(url) = binance::archive_url(market, Interval::M1, period) else { return Ok(None) };
        let Some(zip) = self.get(&url).await? else { return Ok(None) };
        if let Some(sum) = self.get(&format!("{url}.CHECKSUM")).await? {
            let expected = binance::parse_checksum(&String::from_utf8_lossy(&sum));
            if expected.as_deref() != Some(tradoor_store::sha256_hex(&zip).as_str()) {
                return Err(IngestError::Checksum(url));
            }
        }
        let csv = tokio::task::spawn_blocking(move || -> Result<String, IngestError> {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| IngestError::Archive(e.to_string()))?;
            let mut file = archive.by_index(0).map_err(|e| IngestError::Archive(e.to_string()))?;
            let mut text = String::new();
            file.read_to_string(&mut text)?;
            Ok(text)
        })
        .await
        .map_err(|e| IngestError::Archive(e.to_string()))??;
        stats.archives += 1;
        let mut klines = binance::parse_archive_csv(&csv)?;
        dedupe(&mut klines);
        stats.bars_fetched += klines.len() as u64;
        tracing::info!(%market, period = %period.name(), bars = klines.len(), "archive");
        Ok(Some(to_ticks(Interval::M1, &klines)?))
    }

    fn rest_base(&self, market: &MarketRef) -> &str {
        match (market.venue.exchange(), market.venue.kind()) {
            (Exchange::Bybit, _) => &self.config.bybit_rest,
            (Exchange::Binance, tradoor_venues::Kind::Spot) => &self.config.binance_spot_rest,
            (Exchange::Binance, tradoor_venues::Kind::Perp) => &self.config.binance_perp_rest,
        }
    }

    /// Klines for `[from, to)`, in windows no larger than one response.
    async fn rest(&self, market: &MarketRef, interval: Interval, from: i64, to: i64, stats: &mut SyncStats) -> Result<Option<TickCandles>, IngestError> {
        let step = interval.ms().expect("REST paging is only used for fixed intervals");
        let limit = match market.venue.exchange() {
            Exchange::Binance => binance::max_klines(market.venue),
            Exchange::Bybit => bybit::MAX_KLINES,
        };
        let mut parts = Vec::new();
        let mut cursor = from;
        while cursor < to {
            let end = (cursor + i64::from(limit) * step).min(to);
            let base = self.rest_base(market);
            let url = match market.venue.exchange() {
                Exchange::Binance => binance::klines_url(base, market, interval, Some(cursor), Some(end - 1), limit),
                Exchange::Bybit => bybit::klines_url(base, market, interval, Some(cursor), Some(end - 1), limit),
            };
            tokio::time::sleep(self.config.pace).await;
            stats.rest_requests += 1;
            let body = self.get(&url).await?.ok_or_else(|| IngestError::Http(format!("404 from {url}")))?;
            let body = String::from_utf8_lossy(&body);
            let mut klines = match market.venue.exchange() {
                Exchange::Binance => binance::parse_klines(&body)?,
                Exchange::Bybit => bybit::parse_klines(&body)?,
            };
            klines.retain(|k| k.open_time >= cursor && k.open_time < end);
            dedupe(&mut klines);
            stats.bars_fetched += klines.len() as u64;
            if !klines.is_empty() {
                parts.push(to_ticks(interval, &klines)?);
            }
            cursor = end;
        }
        Ok(TickCandles::merge(&parts)?)
    }

    /// The first day the exchange has any bar for, at or after `from`. Asking
    /// for years before a listing would otherwise cost a request per empty day.
    async fn first_day(&self, market: &MarketRef, from: i64, to: i64, stats: &mut SyncStats) -> Result<Option<i64>, IngestError> {
        let window = 900 * DAY;
        let mut cursor = from;
        while cursor < to {
            let end = (cursor + window).min(to);
            if let Some(days) = self.rest(market, Interval::D1, cursor, end, stats).await? {
                return Ok(days.open_time.first().copied());
            }
            cursor = end;
        }
        Ok(None)
    }

    /// Closed 1m bars for `[lo, hi)` inside `period`: day files already on disk,
    /// then the archive, then REST for whatever is still missing.
    async fn minutes(&self, market: &MarketRef, period: Period, lo: i64, hi: i64, now: i64, stats: &mut SyncStats, db: &mut Option<&mut Db>) -> Result<TickCandles, IngestError> {
        let closed_hi = hi.min(Interval::M1.align(now));
        let mut parts = Vec::new();
        if matches!(period, Period::Month(..)) {
            for day in day_periods(period.start(), period.end()) {
                if let Some(t) = self.data.read(market, Interval::M1, day)? {
                    parts.push(t);
                }
            }
        }
        if market.venue.exchange() == Exchange::Binance {
            match self.archive(market, period, stats).await? {
                Some(t) => parts.push(t),
                // A month that ended days ago may not be archived yet, while its
                // days already are.
                None if matches!(period, Period::Month(..)) && period.end() > now - 10 * DAY => {
                    for day in day_periods(lo, closed_hi) {
                        if let Some(t) = self.archive(market, day, stats).await? {
                            parts.push(t);
                        }
                    }
                }
                None => {}
            }
        }
        let have = merged(&parts)?.slice(lo, closed_hi);
        let mut parts = vec![have.clone()];
        for (a, b) in missing_ranges(&have, lo, closed_hi) {
            if let Some(t) = self.rest(market, Interval::M1, a, b, stats).await? {
                parts.push(t);
            }
        }
        let have = merged(&parts)?.slice(lo, closed_hi);
        if let Some(db) = db.as_deref_mut() {
            for (a, b) in interior_gaps(&have) {
                db.record_gap(market, "1m", a, b, "the exchange returned no bars").await?;
                stats.gaps += 1;
            }
        }
        Ok(have)
    }

    /// Bring one market's files up to date: every closed minute since `from`,
    /// rolled up into hours and days, and the manifest rebuilt. Idempotent — a
    /// period already on disk costs a `stat`, not a request.
    pub async fn sync_market(&self, market: &MarketRef, from: i64, now: i64, mut db: Option<&mut Db>) -> Result<SyncStats, IngestError> {
        let mut stats = SyncStats::default();
        let today = Interval::D1.align(now);
        let requested = Interval::D1.align(from);
        if requested >= today {
            return Ok(stats);
        }
        // Probe for the listing only across a range nothing on disk covers: the
        // years before a market existed would otherwise cost a request per empty
        // day, on every pass.
        let kept = self.data.periods(market, Interval::M1)?.first().map(|p| p.start());
        let from = match kept {
            Some(start) if requested >= start => requested,
            _ => match self.first_day(market, requested, kept.unwrap_or(today), &mut stats).await? {
                Some(listed) => listed.max(requested),
                None => match kept {
                    Some(start) => start,
                    None => {
                        tracing::warn!(%market, "the exchange has no bars in the requested range");
                        return Ok(stats);
                    }
                },
            },
        };

        let mut month = Period::containing(from, Tier::Month);
        while month.start() < today {
            if month.is_closed(now) {
                if !self.data.exists(market, Interval::M1, month) {
                    let ticks = self.minutes(market, month, from.max(month.start()), month.end(), now, &mut stats, &mut db).await?;
                    if !ticks.is_empty() {
                        self.data.write(market, Interval::M1, month, &ticks)?;
                        stats.files_written += 1;
                    }
                    for day in day_periods(month.start(), month.end()) {
                        self.data.remove(market, Interval::M1, day)?;
                    }
                }
            } else {
                for day in day_periods(from.max(month.start()), today) {
                    if !self.data.exists(market, Interval::M1, day) {
                        let ticks = self.minutes(market, day, day.start(), day.end(), now, &mut stats, &mut db).await?;
                        if !ticks.is_empty() {
                            self.data.write(market, Interval::M1, day, &ticks)?;
                            stats.files_written += 1;
                        }
                    }
                }
            }
            month = next_month(month);
        }

        for base in [Interval::H1, Interval::D1] {
            self.roll_up(market, base, from, today, now, &mut stats)?;
        }

        let manifest = self.data.write_manifest(market)?;
        if let Some(db) = db.as_deref_mut() {
            db.sync_partitions(market, &manifest).await?;
        }
        tracing::info!(%market, ?stats, "synced");
        Ok(stats)
    }

    /// Is `target` missing, or older than any 1m file it is built from?
    fn stale(&self, market: &MarketRef, base: Interval, target: Period, sources: &[Period]) -> bool {
        match self.data.modified(market, base, target) {
            None => true,
            Some(built) => sources.iter().any(|s| self.data.modified(market, Interval::M1, *s).is_some_and(|m| m > built)),
        }
    }

    fn aggregate(&self, market: &MarketRef, base: Interval, sources: &[Period]) -> Result<Option<TickCandles>, IngestError> {
        let mut parts = Vec::new();
        for s in sources {
            if let Some(t) = self.data.read(market, Interval::M1, *s)? {
                parts.push(t.aggregate(base));
            }
        }
        Ok(TickCandles::merge(&parts)?)
    }

    /// Hours and days from the stored minutes: a year file per closed year, a
    /// month file per closed month of this year, a day file per closed day of
    /// this month — and the finer files a coarser one replaces are removed.
    fn roll_up(&self, market: &MarketRef, base: Interval, from: i64, today: i64, now: i64, stats: &mut SyncStats) -> Result<(), IngestError> {
        let (first_year, this_year) = (Period::containing(from, Tier::Year), Period::containing(now, Tier::Year));
        let Period::Year(y0) = first_year else { unreachable!() };
        let Period::Year(y1) = this_year else { unreachable!() };
        for y in y0..=y1 {
            let year = Period::Year(y);
            let months: Vec<Period> = (1..=12).map(|m| Period::Month(y, m)).filter(|m| m.start() < today).collect();
            if year.is_closed(now) {
                let sources: Vec<Period> = months.iter().copied().filter(|m| self.data.exists(market, Interval::M1, *m)).collect();
                if sources.is_empty() || !self.stale(market, base, year, &sources) {
                    continue;
                }
                if let Some(ticks) = self.aggregate(market, base, &sources)? {
                    self.data.write(market, base, year, &ticks)?;
                    stats.files_written += 1;
                }
                for m in &months {
                    self.data.remove(market, base, *m)?;
                }
                continue;
            }
            for month in months {
                if month.is_closed(now) {
                    if self.data.exists(market, Interval::M1, month) && self.stale(market, base, month, &[month]) {
                        if let Some(ticks) = self.aggregate(market, base, &[month])? {
                            self.data.write(market, base, month, &ticks)?;
                            stats.files_written += 1;
                        }
                        for day in day_periods(month.start(), month.end()) {
                            self.data.remove(market, base, day)?;
                        }
                    }
                } else {
                    for day in day_periods(month.start(), today) {
                        if self.data.exists(market, Interval::M1, day) && self.stale(market, base, day, &[day]) {
                            if let Some(ticks) = self.aggregate(market, base, &[day])? {
                                self.data.write(market, base, day, &ticks)?;
                                stats.files_written += 1;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minutes(times: &[i64]) -> TickCandles {
        let mut t = TickCandles::empty(Interval::M1);
        for &x in times {
            t.open_time.push(x * MINUTE);
            t.open.push(1);
            t.high.push(1);
            t.low.push(1);
            t.close.push(1);
            t.volume.push(1);
        }
        t
    }

    #[test]
    fn missing_and_interior_ranges() {
        let have = minutes(&[2, 3, 6]);
        assert_eq!(missing_ranges(&have, 0, 8 * MINUTE), vec![(0, 2 * MINUTE), (4 * MINUTE, 6 * MINUTE), (7 * MINUTE, 8 * MINUTE)]);
        assert_eq!(interior_gaps(&have), vec![(4 * MINUTE, 6 * MINUTE)]);
        assert_eq!(missing_ranges(&TickCandles::empty(Interval::M1), 0, MINUTE), vec![(0, MINUTE)]);
    }

    #[test]
    fn day_periods_cover_the_range() {
        let days: Vec<Period> = day_periods(0, 3 * DAY).collect();
        assert_eq!(days, vec![Period::Day(1970, 1, 1), Period::Day(1970, 1, 2), Period::Day(1970, 1, 3)]);
    }
}
