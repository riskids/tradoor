//! What the chart draws besides the reader's own studies: the EMA 12/25 ribbon
//! and its crosses, EMA 200, the prior-session market profile, and TPO sessions.

use crate::indicators::profile::{profile_by_session, session_start_back, ProfileBasis, ProfileOptions};
use crate::indicators::tpo::{suggest_row_size, tpo_by_session, TpoOptions, TpoSession};
use crate::indicators::{cross_over, cross_under, ema, Series};
use crate::{Candle, ChartTimeframe, Interval};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cross {
    /// Index into the candles — the bar the cross completed on.
    pub index: usize,
    pub bull: bool,
}

pub struct EngineSeries {
    pub ema12: Series,
    pub ema25: Series,
    pub ema200: Series,
    /// Raw EMA 12/25 crossings, by the same strict definition `ta.crossover` uses.
    pub crosses: Vec<Cross>,
}

pub fn engine_series(candles: &[Candle]) -> EngineSeries {
    let close: Series = candles.iter().map(|c| Some(c.close)).collect();
    let (ema12, ema25) = (ema(&close, 12), ema(&close, 25));
    let (up, down) = (cross_over(&ema12, &ema25), cross_under(&ema12, &ema25));
    let crosses = (0..candles.len())
        .filter_map(|i| match (up[i], down[i]) {
            (Some(true), _) => Some(Cross { index: i, bull: true }),
            (_, Some(true)) => Some(Cross { index: i, bull: false }),
            _ => None,
        })
        .collect();
    EngineSeries { ema200: ema(&close, 200), ema12, ema25, crosses }
}

/// Which session a timeframe's profile describes, and which candles build it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileSpec {
    pub session: ChartTimeframe,
    pub from: Interval,
}

/// Intraday charts profile the prior DAY from hourly bars, daily ones the prior
/// WEEK from dailies, weekly ones the prior MONTH from dailies — the same ladder
/// the rules engine used. Monthly and up have no session above them.
pub fn profile_spec_for(tf: ChartTimeframe) -> Option<ProfileSpec> {
    use ChartTimeframe::*;
    match tf {
        M1 | M5 | M15 | M30 | H1 | H2 | H4 | H6 | H12 => Some(ProfileSpec { session: D1, from: Interval::H1 }),
        D1 | D2 | D3 => Some(ProfileSpec { session: W1, from: Interval::D1 }),
        W1 | W2 => Some(ProfileSpec { session: Mo1, from: Interval::D1 }),
        Mo1 | Mo3 | Mo6 | Mo12 => None,
    }
}

/// A week holds 5–7 daily bars and a holiday week 4; the rules asked for 4.
pub const PROFILE_MIN_BARS: usize = 4;

pub struct ProfileOverlay {
    pub vah: Series,
    pub val: Series,
    pub poc: Series,
    /// Bars with a level. Usually partial: the source candles rarely reach as
    /// far back as the chart does, and a missing level is `None`, never a stale
    /// value carried back through history.
    pub covered: usize,
}

/// Prior-session levels, index-aligned with `candles`. The level on bar i comes
/// from the session BEFORE the one bar i sits in, so it can never move.
pub fn profile_overlay(candles: &[Candle], source: &[Candle], spec: ProfileSpec, basis: ProfileBasis) -> ProfileOverlay {
    let n = candles.len();
    let mut out = ProfileOverlay { vah: vec![None; n], val: vec![None; n], poc: vec![None; n], covered: 0 };
    if source.is_empty() {
        return out;
    }
    let profiles = profile_by_session(source, spec.session, &ProfileOptions { basis, ..Default::default() }, PROFILE_MIN_BARS);
    for (i, c) in candles.iter().enumerate() {
        if let Some(p) = profiles.get(&session_start_back(c.open_time, spec.session, 1)) {
            out.vah[i] = Some(p.vah);
            out.val[i] = Some(p.val);
            out.poc[i] = Some(p.poc);
            out.covered += 1;
        }
    }
    out
}

/// Rows a TPO profile is divided into; fixes the pane's height regardless of
/// whether the instrument trades at 0.0001 or 100000.
pub const TPO_TARGET_ROWS: usize = 46;

pub struct TpoChartData {
    pub spec: ProfileSpec,
    pub row_size: f64,
    pub grid_low: f64,
    pub sessions: Vec<TpoSession>,
    /// Why nothing could be drawn, when nothing could.
    pub reason: Option<String>,
}

/// TPO sessions for the letter chart. The window is taken by SESSION (a half
/// session profiles cleanly and describes something that never happened), then
/// the grid is rebuilt over the kept sessions' own range so recent sessions are
/// not squashed by years of price.
pub fn compute_tpo(source: &[Candle], spec: ProfileSpec, max_sessions: usize) -> TpoChartData {
    let empty = |reason: &str| TpoChartData { spec, row_size: 0.0, grid_low: 0.0, sessions: Vec::new(), reason: Some(reason.to_string()) };
    if source.is_empty() {
        return empty("no candles to build sessions from");
    }
    let probe = suggest_row_size(source, TPO_TARGET_ROWS);
    if !(probe > 0.0) {
        return empty("every candle sits at one price, so there is no distribution to draw");
    }
    let grid_low = source.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
    let all = tpo_by_session(source, spec.session, &TpoOptions::new(probe, grid_low));
    let Some(first_kept) = all.len().checked_sub(max_sessions.max(1)).map_or(all.first(), |skip| all.get(skip)) else {
        return empty("no complete session could be built from these candles");
    };
    let windowed: Vec<Candle> = source.iter().filter(|c| c.open_time >= first_kept.session_start).copied().collect();
    let row_size = suggest_row_size(&windowed, TPO_TARGET_ROWS);
    if !(row_size > 0.0) {
        return empty("the visible window has no price range to divide into rows");
    }
    let low = windowed.iter().map(|c| c.low).fold(f64::INFINITY, f64::min);
    TpoChartData {
        spec,
        row_size,
        grid_low: low,
        sessions: tpo_by_session(&windowed, spec.session, &TpoOptions::new(row_size, low)),
        reason: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{parse_iso, DAY};

    #[test]
    fn crosses_are_found_by_index() {
        let closes: Vec<f64> = (0..120).map(|i| 100.0 + ((i as f64) / 9.0).sin() * 10.0).collect();
        let candles = crate::fixtures::candles_from_closes(&closes, 1.0);
        let s = engine_series(&candles);
        assert!(!s.crosses.is_empty());
        for c in &s.crosses {
            let (f, sl) = (s.ema12[c.index].unwrap(), s.ema25[c.index].unwrap());
            assert_eq!(c.bull, f > sl);
        }
        assert_eq!(crate::fixtures::warmup_length(&s.ema200), 120);
    }

    #[test]
    fn tpo_keeps_the_newest_sessions() {
        let start = parse_iso("2026-08-03").unwrap();
        let candles: Vec<Candle> = (0..60)
            .map(|i| {
                let p = 100.0 + (i as f64 / 5.0).sin() * 5.0;
                Candle { open_time: start + i * DAY, open: p, high: p + 2.0, low: p - 2.0, close: p, volume: 1.0, quote_volume: None }
            })
            .collect();
        let tpo = compute_tpo(&candles, profile_spec_for(ChartTimeframe::D1).unwrap(), 4);
        assert!(tpo.reason.is_none());
        assert_eq!(tpo.sessions.len(), 4);
        for s in &tpo.sessions {
            assert!(s.val <= s.poc && s.poc <= s.vah);
        }
    }
}
