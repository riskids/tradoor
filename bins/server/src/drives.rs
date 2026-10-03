//! Three drives: three pushes to one rising line, each a little past the last,
//! a real pullback between them — and then Stochastic RSI turning down out of
//! overbought. The bullish pattern is the same thing upside down: three lower
//! lows along a falling line, and the turn up out of oversold.
//!
//! Only closed bars are read, and a pivot is only taken once the bars after it
//! that make it one have closed, so what is found does not change with the
//! forming bar. The bullish search runs on the bars mirrored — highs and lows
//! swapped and every price negated — which turns RSI and Stochastic RSI into
//! a hundred less themselves, so one search serves both sides.

use tradoor_core::indicators::{atr, pivot_highs, rsi, stoch_rsi};
use tradoor_core::Candle;

/// Bars either side of a high that make it a drive: five before, and two after
/// so the newest drive is known eight hours on, not a day.
const PIVOT_LEFT: usize = 5;
const PIVOT_RIGHT: usize = 2;
/// Bars at least between two drives: a day of four-hour bars.
const MIN_GAP: usize = 6;
/// Bars at most from the first drive to the third: forty days.
const MAX_SPAN: usize = 240;
/// How deep the pullback between two drives goes, at least, in ATRs —
const PULLBACK_ATR: f64 = 2.0;
/// — and as a part of the push that made the drive before it.
const RETRACE: f64 = 0.382;
/// How far each drive clears the last, at least, in ATRs: less is a double top.
const CLEAR_ATR: f64 = 0.5;
/// How far a bar between the drives may poke through their line, in ATRs.
const LINE_ATR: f64 = 0.5;
/// How far the third drive may sit from the line through the first two, in ATRs.
const ALIGN_ATR: f64 = 1.0;
/// How far a close after the third drive may go through the line before the
/// pattern is broken, in ATRs.
const BREAK_ATR: f64 = 1.0;
/// Bars a third drive waits for the turn: two days.
const WAIT_BARS: usize = 12;
/// Bars a turn stays news: two days.
const KEEP_BARS: usize = 12;
/// Where Stochastic RSI has to have been, in the bars before it turns.
const OVERBOUGHT: f64 = 80.0;
/// Drives looked back over for the first two.
const LOOK_BACK: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Three higher highs, then the turn down.
    Bear,
    /// Three lower lows, then the turn up.
    Bull,
}

impl Side {
    pub fn id(self) -> &'static str {
        match self {
            Side::Bear => "bear",
            Side::Bull => "bull",
        }
    }
}

/// A bar of the pattern: a drive's extreme, or the bar the turn came on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub index: usize,
    pub at: i64,
    pub price: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub side: Side,
    pub drives: [Point; 3],
    /// The bar Stochastic RSI turned on, once it has; its close.
    pub turn: Option<Point>,
    /// RSI weaker at the third drive than at the second.
    pub divergence: bool,
    /// 0 to 100: how well the third drive sits on the line, the divergence,
    /// how evenly the drives are spaced, volume fading, and the turn.
    pub score: u32,
    /// Where the pattern says price goes: back to the pullback before the
    /// third drive — its low for the bearish pattern, its high for the bullish.
    pub target: f64,
    /// Where it is wrong: a close past the third drive.
    pub stop: f64,
}

/// The pattern on `side` whose third drive is the newest, if there is one now.
pub fn find(candles: &[Candle], side: Side) -> Option<Found> {
    let prepared = Prepared::new(candles, side)?;
    prepared.at(candles.len() - 1)
}

/// Every pattern the bars held, each as a scan would have reported it the
/// first time it had turned — the bar that scan read up to, and what it saw.
/// Nothing after that bar is looked at to find it.
pub fn history(candles: &[Candle], side: Side) -> Vec<(usize, Found)> {
    let Some(p) = Prepared::new(candles, side) else { return Vec::new() };
    let mut told = std::collections::HashSet::new();
    // A scan at every bar, as the live ones run, each pattern taken the first time one sees it turned.
    (0..candles.len()).filter_map(|at| p.at(at).filter(|f| f.turn.is_some() && told.insert(f.drives[2].index)).map(|f| (at, f))).collect()
}

/// How a pattern played out after it was reported at `entry`: target or stop
/// first, or neither within the window. `after` is the bars that followed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outcome {
    pub result: Hit,
    /// Bars until it was decided, or the bars looked at.
    pub bars: usize,
    /// The move from entry to the exit, in percent, positive the pattern's way.
    pub ret: f64,
    /// The furthest it went the pattern's way, and against, in percent.
    pub best: f64,
    pub worst: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Target,
    Stop,
    /// Neither, within the window.
    Expired,
    /// Not enough bars yet to say.
    Open,
    /// Price was already past the target when the pattern turned.
    Passed,
}

impl Hit {
    pub fn id(self) -> &'static str {
        match self {
            Hit::Target => "target",
            Hit::Stop => "stop",
            Hit::Expired => "expired",
            Hit::Open => "open",
            Hit::Passed => "passed",
        }
    }
}

/// Bars a pattern is given to reach its target: four days.
pub const OUTCOME_BARS: usize = 24;

pub fn outcome(side: Side, entry: f64, target: f64, stop: f64, after: &[Candle]) -> Outcome {
    let bear = side == Side::Bear;
    let pct = |p: f64| if bear { (entry - p) / entry * 100.0 } else { (p - entry) / entry * 100.0 };
    let mut out = Outcome { result: Hit::Open, bars: 0, ret: 0.0, best: 0.0, worst: 0.0 };
    if (bear && entry <= target) || (!bear && entry >= target) {
        out.result = Hit::Passed;
        return out;
    }
    for (i, k) in after.iter().take(OUTCOME_BARS).enumerate() {
        let (with, against) = if bear { (k.low, k.high) } else { (k.high, k.low) };
        out.bars = i + 1;
        out.worst = out.worst.min(pct(against));
        // Stopped by a close past the third drive — a wick through it is a
        // retest, not the end — and out at that close. A bar that closes past
        // it after touching the target counts as the stop: which came first
        // is not known.
        if (bear && k.close > stop) || (!bear && k.close < stop) {
            (out.result, out.ret) = (Hit::Stop, pct(k.close));
            return out;
        }
        out.best = out.best.max(pct(with));
        if (bear && with <= target) || (!bear && with >= target) {
            (out.result, out.ret) = (Hit::Target, pct(target));
            return out;
        }
    }
    if out.bars >= OUTCOME_BARS {
        out.result = Hit::Expired;
        out.ret = pct(after[OUTCOME_BARS - 1].close);
    }
    out
}

/// Highs and lows swapped and every price negated: lows become highs.
fn mirror(c: &Candle) -> Candle {
    Candle { open: -c.open, high: -c.low, low: -c.high, close: -c.close, ..*c }
}

/// The geometry of three drives ending at one pivot: which earlier pivots are
/// the first two, how well the third sits on their line, and its pullback.
#[derive(Clone, Copy)]
struct Shape {
    a: usize,
    b: usize,
    z: usize,
    fit: f64,
    /// The extreme of the pullback before the third drive.
    target: f64,
}

/// The bars, as they are or mirrored, with what every search reads of them.
/// Each series is causal — its value at a bar depends on no later bar — so the
/// search can be run as of any bar without looking past it.
struct Prepared {
    side: Side,
    c: Vec<Candle>,
    atr: Vec<Option<f64>>,
    rsi: Vec<Option<f64>>,
    k: Vec<Option<f64>>,
    d: Vec<Option<f64>>,
    pivots: Vec<tradoor_core::indicators::Pivot>,
    /// Each pivot's shape, worked out the first time it is asked for.
    shapes: Vec<std::cell::OnceCell<Option<Shape>>>,
}

impl Prepared {
    fn new(candles: &[Candle], side: Side) -> Option<Self> {
        if candles.len() < 60 {
            return None;
        }
        let c: Vec<Candle> = match side {
            Side::Bear => candles.to_vec(),
            Side::Bull => candles.iter().map(mirror).collect(),
        };
        let high: Vec<Option<f64>> = c.iter().map(|k| Some(k.high)).collect();
        let close: Vec<Option<f64>> = c.iter().map(|k| Some(k.close)).collect();
        let stoch = stoch_rsi(&close, 14, 14, 3, 3);
        let pivots = pivot_highs(&high, PIVOT_LEFT, PIVOT_RIGHT);
        let shapes = (0..pivots.len()).map(|_| std::cell::OnceCell::new()).collect();
        Some(Self { side, atr: atr(&c, 14), rsi: rsi(&close, 14), k: stoch.k, d: stoch.d, pivots, shapes, c })
    }

    fn atr_at(&self, i: usize) -> Option<f64> {
        self.atr[i].filter(|a| *a > 0.0)
    }

    /// The pattern as a scan reading up to `last` would see it: the oldest one
    /// still standing among the drives recent enough to end one. A high since —
    /// a bounce in the pullback, a wick past the third drive, a quiet weekend's
    /// wobble — does not take its place while it stands.
    fn at(&self, last: usize) -> Option<Found> {
        let horizon = last.saturating_sub(WAIT_BARS + KEEP_BARS + PIVOT_RIGHT);
        (0..self.pivots.len())
            .filter(|&zi| self.pivots[zi].index >= horizon && self.pivots[zi].confirmed_at <= last)
            .find_map(|zi| self.judge(self.shapes[zi].get_or_init(|| self.shape(zi)).as_ref()?, last))
    }

    /// Three drives ending at pivot `zi`, if its earlier pivots make them. Only
    /// bars up to the third drive are read.
    fn shape(&self, zi: usize) -> Option<Shape> {
        let c = &self.c;
        let third = self.pivots[zi];
        let z = third.index;
        let atr_z = self.atr_at(z)?;
        let mut best: Option<Shape> = None;
        let earlier = &self.pivots[zi.saturating_sub(LOOK_BACK)..zi];
        for (bi, b) in earlier.iter().enumerate() {
            if b.index + MIN_GAP > z || b.value >= third.value {
                continue;
            }
            let Some(atr_b) = self.atr_at(b.index) else { continue };
            for a in &earlier[..bi] {
                if a.index + MIN_GAP > b.index || z - a.index > MAX_SPAN || a.value >= b.value {
                    continue;
                }
                // Each push clears the last: three drives, not a double top.
                if b.value - a.value < CLEAR_ATR * atr_b || third.value - b.value < CLEAR_ATR * atr_z {
                    continue;
                }
                // The first drive is a fresh high, over everything as far back as the drives are apart.
                let before = &c[a.index.saturating_sub(b.index - a.index)..a.index];
                if before.iter().any(|k| k.high >= a.value) {
                    continue;
                }
                let line = |i: usize| a.value + (b.value - a.value) * (i as f64 - a.index as f64) / (b.index - a.index) as f64;
                // The third drive reaches the line, give or take.
                let off = (third.value - line(z)) / atr_z;
                if off.abs() > ALIGN_ATR {
                    continue;
                }
                // The line holds between the drives: they are pushes to it, not a range under it.
                if (a.index + 1..z).filter(|i| *i != b.index).any(|i| c[i].high > line(i) + LINE_ATR * self.atr_at(i).unwrap_or(atr_z)) {
                    continue;
                }
                // And between them price pulled back for real.
                let low_between = |from: usize, to: usize| c[from + 1..to].iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
                let low_before = before.iter().map(|k| k.low).fold(f64::INFINITY, f64::min);
                let (low_ab, low_bz) = (low_between(a.index, b.index), low_between(b.index, z));
                if a.value - low_ab < (PULLBACK_ATR * atr_b).max(RETRACE * (a.value - low_before))
                    || b.value - low_bz < (PULLBACK_ATR * atr_z).max(RETRACE * (b.value - low_ab))
                {
                    continue;
                }
                let fit = 1.0 - off.abs() / ALIGN_ATR;
                if best.as_ref().is_none_or(|s| fit > s.fit) {
                    best = Some(Shape { a: a.index, b: b.index, z, fit, target: low_bz });
                }
            }
        }
        best
    }

    /// A shape as of `last`: still standing, turned or still waiting, scored.
    fn judge(&self, shape: &Shape, last: usize) -> Option<Found> {
        let c = &self.c;
        let Shape { a, b, z, fit, target } = *shape;
        let top = c[z].high;
        // Outdone: a close past the third drive. A wick past it is a retest.
        if c[z + 1..=last].iter().any(|k| k.close > top) {
            return None;
        }
        let atr_z = self.atr_at(z)?;
        let line = |i: usize| c[a].high + (c[b].high - c[a].high) * (i as f64 - a as f64) / (b - a) as f64;
        // Broken: a close well through the line since the third drive.
        if (z + 1..=last).any(|i| c[i].close > line(i) + BREAK_ATR * self.atr_at(i).unwrap_or(atr_z)) {
            return None;
        }
        let turn = turn_down(&self.k[..=last], &self.d[..=last], z);
        match turn {
            Some(t) if last - t > KEEP_BARS => return None,
            None if last - z > WAIT_BARS => return None,
            _ => {}
        }

        let (ra, rb, rz) = (self.rsi[a].unwrap_or(50.0), self.rsi[b].unwrap_or(50.0), self.rsi[z].unwrap_or(50.0));
        let divergence = rz < rb;
        let (gap1, gap2) = ((b - a) as f64, (z - b) as f64);
        let volume = |i: usize| c[i.saturating_sub(2)..=(i + 2).min(last)].iter().map(|k| k.volume).sum::<f64>();
        let mut score = 30.0 * fit + 15.0 * gap1.min(gap2) / gap1.max(gap2);
        if divergence {
            score += 15.0;
            if rb < ra {
                score += 10.0;
            }
        }
        if volume(z) < volume(b) {
            score += 15.0;
        }
        if turn.is_some() {
            score += 15.0;
        }

        // Prices back the right way up for the bullish side.
        let sign = if self.side == Side::Bull { -1.0 } else { 1.0 };
        let point = |i: usize, price: f64| Point { index: i, at: c[i].open_time, price: sign * price };
        Some(Found {
            side: self.side,
            drives: [point(a, c[a].high), point(b, c[b].high), point(z, top)],
            turn: turn.map(|t| point(t, c[t].close)),
            divergence,
            score: score.round().clamp(0.0, 100.0) as u32,
            target: sign * target,
            stop: sign * top,
        })
    }
}

/// The first bar from `from` on where K crosses under D, having been
/// overbought in the three bars before.
fn turn_down(k: &[Option<f64>], d: &[Option<f64>], from: usize) -> Option<usize> {
    (from.max(3)..k.len()).find(|&t| {
        let crossed = matches!((k[t - 1], d[t - 1], k[t], d[t]), (Some(k0), Some(d0), Some(k1), Some(d1)) if k0 >= d0 && k1 < d1);
        crossed && k[t - 3..t].iter().flatten().any(|v| *v >= OVERBOUGHT)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const H4: i64 = 4 * 3_600_000;

    /// Bars along straight legs between `(index, price)` corners, with a small
    /// wobble so RSI moves; the highs half a point over the close, the lows under.
    fn legs(corners: &[(usize, f64)]) -> Vec<Candle> {
        let mut closes = Vec::new();
        for pair in corners.windows(2) {
            let ((i0, p0), (i1, p1)) = (pair[0], pair[1]);
            for i in i0..i1 {
                closes.push(p0 + (p1 - p0) * (i - i0) as f64 / (i1 - i0) as f64);
            }
        }
        closes.push(corners.last().unwrap().1);
        let peaks: Vec<usize> = corners.iter().map(|(i, _)| *i).collect();
        closes
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                // Corners stay clean, so each peak is one bar.
                let close = if peaks.contains(&i) { p } else { p + if i % 2 == 0 { 0.15 } else { -0.15 } };
                Candle { open_time: i as i64 * H4, open: close, high: close + 0.5, low: close - 0.5, close, volume: 100.0, quote_volume: None }
            })
            .collect()
    }

    /// Three pushes to a line rising a sixth of a point a bar — 110, 115, 120 —
    /// with ten-point pullbacks, then the fall that follows.
    fn three_up(after: &[(usize, f64)]) -> Vec<Candle> {
        let mut corners = vec![(0, 95.0), (60, 110.0), (75, 100.0), (90, 115.0), (105, 105.0), (120, 120.0)];
        corners.extend_from_slice(after);
        legs(&corners)
    }

    #[test]
    fn three_pushes_to_one_line_are_found() {
        let bars = three_up(&[(126, 114.0)]);
        let found = find(&bars, Side::Bear).expect("three drives");
        assert_eq!(found.drives.map(|d| d.index), [60, 90, 120]);
        assert_eq!(found.drives.map(|d| d.price), [110.5, 115.5, 120.5]);
        assert_eq!(found.drives[2].at, 120 * H4);
        assert!(found.score >= 30, "on the line, evenly spaced: {}", found.score);
        if let Some(turn) = found.turn {
            assert!(turn.index >= 120, "the turn comes after the third drive");
        }
    }

    #[test]
    fn the_history_reports_a_pattern_as_a_scan_would_have_then() {
        let bars = three_up(&[(126, 114.0), (150, 104.0)]);
        let reported = history(&bars, Side::Bear);
        assert_eq!(reported.len(), 1);
        let (at, found) = &reported[0];
        assert_eq!(*at, 122, "the first scan after the third drive is known, the turn already in");
        assert_eq!(find(&bars[..=*at], Side::Bear).as_ref(), Some(found), "the same as that scan found, seeing nothing later");
        assert_eq!((found.target, found.stop), (104.5, 120.5), "back to the last pullback; wrong past the third drive");
    }

    #[test]
    fn an_outcome_is_the_target_or_the_stop_whichever_comes_first() {
        let bar = |high: f64, low: f64| Candle { open_time: 0, open: low, high, low, close: (high + low) / 2.0, volume: 1.0, quote_volume: None };
        let won = outcome(Side::Bear, 116.0, 104.5, 120.5, &[bar(118.0, 112.0), bar(113.0, 104.0)]);
        assert_eq!((won.result, won.bars), (Hit::Target, 2));
        assert!((won.ret - (116.0 - 104.5) / 116.0 * 100.0).abs() < 1e-9);
        let closing = |high: f64, low: f64, close: f64| Candle { close, ..bar(high, low) };
        assert_eq!(outcome(Side::Bear, 116.0, 104.5, 120.5, &[closing(122.0, 104.0, 121.0)]).result, Hit::Stop, "the target touched, then a close past the stop: not knowing which came first, the stop");
        let retest = outcome(Side::Bear, 116.0, 104.5, 120.5, &[closing(121.5, 115.0, 117.0), bar(113.0, 104.0)]);
        assert_eq!(retest.result, Hit::Target, "a wick past the third drive is a retest, not the stop");
        let stopped = outcome(Side::Bear, 116.0, 104.5, 120.5, &[closing(123.0, 117.0, 122.0)]);
        assert!((stopped.ret - (116.0 - 122.0) / 116.0 * 100.0).abs() < 1e-9, "out at the close that stopped it");
        let flat = vec![bar(117.0, 115.0); OUTCOME_BARS];
        let expired = outcome(Side::Bear, 116.0, 104.5, 120.5, &flat);
        assert_eq!((expired.result, expired.ret), (Hit::Expired, 0.0));
        assert_eq!(outcome(Side::Bear, 116.0, 104.5, 120.5, &flat[..3]).result, Hit::Open, "too soon to say");
        assert_eq!(outcome(Side::Bear, 104.0, 104.5, 120.5, &flat).result, Hit::Passed);
        assert_eq!(outcome(Side::Bull, 102.0, 111.0, 95.0, &[bar(112.0, 101.0)]).result, Hit::Target, "the bullish way up");
    }

    #[test]
    fn the_bullish_one_is_the_same_upside_down() {
        let bars: Vec<Candle> = three_up(&[(126, 114.0)])
            .iter()
            .map(|c| Candle { open: 300.0 - c.open, high: 300.0 - c.low, low: 300.0 - c.high, close: 300.0 - c.close, ..*c })
            .collect();
        let found = find(&bars, Side::Bull).expect("three drives down");
        assert_eq!(found.side, Side::Bull);
        assert_eq!(found.drives.map(|d| d.price), [189.5, 184.5, 179.5], "the lows, in prices as they are");
        assert!(find(&bars, Side::Bear).is_none(), "and nothing bearish in it");
    }

    #[test]
    fn a_third_drive_off_the_line_is_not_one() {
        // The third push overshoots the line by five points: a breakout, not a drive.
        let corners = [(0, 95.0), (60, 110.0), (75, 100.0), (90, 115.0), (105, 105.0), (120, 125.0), (126, 118.0)];
        assert!(find(&legs(&corners), Side::Bear).is_none());
    }

    #[test]
    fn a_new_high_or_a_close_through_the_line_ends_it() {
        // Back up past the third drive: a fourth push, and the three are history.
        assert!(find(&three_up(&[(124, 116.0), (128, 121.5)]), Side::Bear).is_none());
        // Without the pullbacks there is a trend, not three drives.
        let shallow = [(0, 95.0), (60, 110.0), (75, 109.5), (90, 115.0), (105, 114.5), (120, 120.0), (126, 114.0)];
        assert!(find(&legs(&shallow), Side::Bear).is_none());
    }

    #[test]
    fn a_smaller_high_after_the_third_drive_does_not_hide_it() {
        // A bounce in the pullback makes a high of its own, under the third drive.
        let bars = three_up(&[(126, 114.0), (129, 116.8), (131, 115.0)]);
        assert!(Prepared::new(&bars, Side::Bear).unwrap().pivots.iter().any(|p| p.index == 129), "the bounce is a high of its own");
        let found = find(&bars, Side::Bear).expect("still three drives");
        assert_eq!(found.drives[2].index, 120);
    }

    #[test]
    fn a_wick_past_the_third_drive_leaves_it_standing() {
        let mut bars = three_up(&[(126, 114.0)]);
        bars[123].high = 122.0;
        let found = find(&bars, Side::Bear).expect("a wick is a retest");
        assert_eq!(found.drives[2].index, 120);
        // A close past it, well off the line: not a drive on it either.
        (bars[123].high, bars[123].close) = (125.0, 124.0);
        assert!(find(&bars, Side::Bear).is_none(), "a close past it ends it");
    }

    #[test]
    fn the_first_drive_is_a_fresh_high() {
        // A higher high a week before the first drive: a range, not a run of drives.
        let corners = [(0, 95.0), (35, 112.0), (45, 100.0), (60, 110.0), (75, 100.0), (90, 115.0), (105, 105.0), (120, 120.0), (126, 114.0)];
        assert!(find(&legs(&corners), Side::Bear).is_none());
    }

    #[test]
    fn a_pattern_waits_two_days_for_its_turn() {
        // Three weeks after the third drive it is no longer news.
        assert!(find(&three_up(&[(126, 114.0), (246, 112.0)]), Side::Bear).is_none());
    }

    #[test]
    fn the_turn_is_a_cross_down_from_overbought() {
        let k = [Some(70.0), Some(85.0), Some(90.0), Some(88.0), Some(75.0), Some(60.0)];
        let d = [Some(60.0), Some(75.0), Some(84.0), Some(87.0), Some(82.0), Some(70.0)];
        assert_eq!(turn_down(&k, &d, 0), Some(4), "K under D on the fifth bar, having been over 80");
        let low = k.map(|v| v.map(|v| v - 40.0));
        let low_d = d.map(|v| v.map(|v| v - 40.0));
        assert_eq!(turn_down(&low, &low_d, 0), None, "the same cross down at 40 is not a turn from overbought");
        assert_eq!(turn_down(&k, &d, 5), None, "nor one before the third drive");
    }
}
