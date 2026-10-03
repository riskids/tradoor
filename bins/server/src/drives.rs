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
}

/// The pattern on `side` whose third drive is the newest, if there is one now.
pub fn find(candles: &[Candle], side: Side) -> Option<Found> {
    let bars: Vec<Candle> = match side {
        Side::Bear => candles.to_vec(),
        Side::Bull => candles.iter().map(mirror).collect(),
    };
    let mut found = search(&bars)?;
    found.side = side;
    if side == Side::Bull {
        for p in found.drives.iter_mut().chain(found.turn.iter_mut()) {
            p.price = -p.price;
        }
    }
    Some(found)
}

/// Highs and lows swapped and every price negated: lows become highs.
fn mirror(c: &Candle) -> Candle {
    Candle { open: -c.open, high: -c.low, low: -c.high, close: -c.close, ..*c }
}

/// The bearish search, on bars as they are or mirrored.
fn search(c: &[Candle]) -> Option<Found> {
    let n = c.len();
    if n < 60 {
        return None;
    }
    let last = n - 1;
    let high: Vec<Option<f64>> = c.iter().map(|k| Some(k.high)).collect();
    let close: Vec<Option<f64>> = c.iter().map(|k| Some(k.close)).collect();
    let atr = atr(c, 14);
    let rsi = rsi(&close, 14);
    let stoch = stoch_rsi(&close, 14, 14, 3, 3);
    let pivots = pivot_highs(&high, PIVOT_LEFT, PIVOT_RIGHT);

    // The third drive is the newest, and nothing since has gone past it.
    let third = *pivots.last()?;
    let z = third.index;
    if c[z + 1..].iter().any(|k| k.high >= third.value) {
        return None;
    }
    let atr_at = |i: usize| atr[i].filter(|a| *a > 0.0);
    let atr_z = atr_at(z)?;

    let mut best: Option<(f64, usize, usize)> = None;
    let earlier = &pivots[pivots.len().saturating_sub(LOOK_BACK + 1)..pivots.len() - 1];
    for (bi, b) in earlier.iter().enumerate() {
        if b.index + MIN_GAP > z || b.value >= third.value {
            continue;
        }
        let Some(atr_b) = atr_at(b.index) else { continue };
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
            if (a.index + 1..z).filter(|i| *i != b.index).any(|i| c[i].high > line(i) + LINE_ATR * atr_at(i).unwrap_or(atr_z)) {
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
            if best.is_none_or(|(f, _, _)| fit > f) {
                best = Some((fit, a.index, b.index));
            }
        }
    }
    let (fit, ai, bi) = best?;
    let line = |i: usize| c[ai].high + (c[bi].high - c[ai].high) * (i as f64 - ai as f64) / (bi - ai) as f64;

    // Broken: a close well through the line since the third drive.
    if (z + 1..n).any(|i| c[i].close > line(i) + BREAK_ATR * atr_at(i).unwrap_or(atr_z)) {
        return None;
    }

    let turn = turn_down(&stoch.k, &stoch.d, z);
    match turn {
        Some(t) if last - t > KEEP_BARS => return None,
        None if last - z > WAIT_BARS => return None,
        _ => {}
    }

    let (ra, rb, rz) = (rsi[ai].unwrap_or(50.0), rsi[bi].unwrap_or(50.0), rsi[z].unwrap_or(50.0));
    let divergence = rz < rb;
    let (gap1, gap2) = ((bi - ai) as f64, (z - bi) as f64);
    let volume = |i: usize| c[i.saturating_sub(2)..=(i + 2).min(last)].iter().map(|k| k.volume).sum::<f64>();
    let mut score = 30.0 * fit + 15.0 * gap1.min(gap2) / gap1.max(gap2);
    if divergence {
        score += 15.0;
        if rb < ra {
            score += 10.0;
        }
    }
    if volume(z) < volume(bi) {
        score += 15.0;
    }
    if turn.is_some() {
        score += 15.0;
    }

    let point = |i: usize, price: f64| Point { index: i, at: c[i].open_time, price };
    Some(Found {
        side: Side::Bear,
        drives: [point(ai, c[ai].high), point(bi, c[bi].high), point(z, third.value)],
        turn: turn.map(|t| point(t, c[t].close)),
        divergence,
        score: score.round().clamp(0.0, 100.0) as u32,
    })
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
