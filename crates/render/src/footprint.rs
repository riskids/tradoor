//! Footprint columns: what was bought and what was sold at every price inside
//! a bar, over the bar itself.
//!
//! Read as every footprint chart is read. A bar is a column of rows, one a
//! price level, each row two cells — sold on the left, bought on the right.
//! Every bar's rows sit on ONE price grid, so a level reads straight across
//! the screen; a grid per bar would put neighbouring bars' rows at different
//! prices and there would be nothing to compare. The candle stays underneath:
//! the numbers say where the trade happened, the candle where the price went.
//!
//! A cell is filled against the loudest level of its own bar, so a quiet bar
//! reads as well as the busy one beside it. A cell whose side took far more
//! than the other side took one level away — the diagonal every platform
//! compares, at two and a half times here — is ringed as an imbalance, and the
//! level that traded most as the bar's point of control.
//!
//! The numbers are written only where a cell is big enough to hold them: sixty
//! bars of forty levels is five thousand numbers, which no chart draws sixty
//! times a second. Below that the cells carry it alone, as a heat map does.

use crate::paint::{Align, Baseline, Frame, Rect};
use crate::theme::Theme;

/// How a level is shown: the two sides apart, their difference, or their sum.
/// Named as every footprint chart names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Sold on the left, bought on the right.
    #[default]
    Split,
    /// One cell, green where more was bought than sold and red where less.
    Delta,
    /// One cell: everything that traded at the level, whichever way.
    Total,
    /// No numbers: a bar across the cell, as long as the level was busy.
    Cluster,
    /// The delta added up from the foot of the bar, level by level.
    Ladder,
}

impl Mode {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "delta" => Self::Delta,
            "total" => Self::Total,
            "cluster" => Self::Cluster,
            "ladder" => Self::Ladder,
            _ => Self::Split,
        }
    }
}

/// What else a column carries, beyond its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub mode: Mode,
    /// The rows that hold most of the bar's volume, outlined.
    pub value_area: bool,
    /// What the whole bar came to, written under it.
    pub stats: bool,
    /// Every visible bar summed by price, drawn against the right edge.
    pub profile: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { mode: Mode::default(), value_area: true, stats: true, profile: true }
    }
}

/// A bar's levels: price, coins bought, coins sold — lowest price first.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub time: i64,
    pub levels: Vec<(f64, f64, f64)>,
}

/// Where the numbers start being written, in pixels.
const NUMBER_WIDTH: f32 = 46.0;
const NUMBER_HEIGHT: f32 = 8.0;
/// A cell is never drawn thinner than this, so a busy bar still reads.
const MIN_ROW: f32 = 1.0;
/// How many times the other side a side must take, one level down, to be an
/// imbalance. Exocharts asks two and a half, ATAS one and a half; this is
/// theirs, and the diagonal is everyone's.
const IMBALANCE: f64 = 2.5;
/// A level this quiet against the bar's loudest is noise, whatever its ratio.
const IMBALANCE_FLOOR: f64 = 0.04;
/// Imbalances in a row that make a run worth marking as one.
const STACKED: usize = 3;
/// The share of a bar's volume the value area holds, as every platform has it.
const VALUE_AREA: f64 = 0.7;
/// How wide the profile against the right edge is drawn, as a share of the plot.
const PROFILE_SHARE: f32 = 0.12;

/// What the chart lends the painter: where things go, and what they look like.
pub struct View<'a> {
    pub plot: Rect,
    pub bar_spacing: f32,
    pub char_width: f32,
    pub theme: &'a Theme,
    /// Logical bar index to x, and price to y.
    pub x: &'a dyn Fn(f64) -> f32,
    pub y: &'a dyn Fn(f64) -> f32,
}

/// How a volume is written inside a cell: short, because the room is short —
/// and keeping its sign, since a delta that lost its minus says the opposite.
fn short(v: f64) -> String {
    let sign = if v < 0.0 { "-" } else { "" };
    match v.abs() {
        n if n >= 1_000.0 => format!("{sign}{:.0}K", n / 1_000.0),
        n if n >= 100.0 => format!("{sign}{n:.0}"),
        n if n >= 1.0 => format!("{sign}{n:.1}"),
        n if n >= 0.01 => format!("{sign}{n:.2}"),
        _ => String::new(),
    }
}

/// The bars in view, each at its logical index. `grid` is the price one row
/// covers, the same for every bar on screen.
pub fn paint(f: &mut Frame, view: &View<'_>, bars: &[(f64, &Bar)], options: Options, grid: f64) {
    let t = view.theme;
    let mode = options.mode;
    let weight = |buy: f64, sell: f64| match mode {
        Mode::Split => buy.max(sell),
        Mode::Delta | Mode::Ladder => (buy - sell).abs(),
        Mode::Total | Mode::Cluster => buy + sell,
    };
    if !bars.iter().flat_map(|(_, bar)| bar.levels.iter()).any(|(_, buy, sell)| weight(*buy, *sell) > 0.0) {
        return;
    }
    let width = (view.bar_spacing * 0.86).max(2.0);
    let half = width / 2.0;

    for (logical, bar) in bars {
        if bar.levels.len() < 2 {
            continue;
        }
        let x = (view.x)(*logical);
        if x + half < view.plot.x || x - half > view.plot.right() {
            continue;
        }
        // A cell is filled against the loudest level of ITS OWN bar. Against
        // the loudest on screen, a busy minute an hour ago left every level of
        // every quiet bar the same near-invisible wash.
        let loudest = bar.levels.iter().map(|(_, buy, sell)| weight(*buy, *sell)).fold(0.0_f64, f64::max);
        if loudest <= 0.0 {
            continue;
        }
        // One grid for the screen, so a level reads straight across it.
        let step = if grid > 0.0 { grid } else { level_step(bar) };
        let row = row_height(view, bar.levels[bar.levels.len() / 2].0, step);
        let numbers = width >= NUMBER_WIDTH && row >= NUMBER_HEIGHT;
        let size = row.min(11.0) * 0.82;
        let pad = 3.0;
        let top_of = |price: f64| (view.y)(price + step);
        let full_at = |price: f64| row_height(view, price, step);
        let drawn_at = |price: f64| (full_at(price) - 0.5).max(MIN_ROW);
        let on_screen = |price: f64| top_of(price) + full_at(price) >= view.plot.y && top_of(price) <= view.plot.bottom();

        // The delta added up from the foot of the bar, for the ladder.
        let mut running = 0.0;
        for (price, buy, sell) in &bar.levels {
            running += buy - sell;
            if !on_screen(*price) {
                continue;
            }
            let (top, drawn) = (top_of(*price), drawn_at(*price));
            let alpha = |v: f64| (55.0 + 200.0 * (v / loudest).clamp(0.0, 1.0).sqrt() as f32) as u8;
            let side = if buy >= sell { t.bull } else { t.bear };
            match mode {
                Mode::Split => {
                    f.rect(x - half, top, half, drawn, t.bear.with_alpha(alpha(*sell)));
                    f.rect(x, top, half, drawn, t.bull.with_alpha(alpha(*buy)));
                    if numbers {
                        // Each number in the colour of the side it belongs to.
                        f.text(x - pad, top + drawn / 2.0, short(*sell), t.bear, Align::Right, Baseline::Middle, size, None, Some(view.plot));
                        f.text(x + pad, top + drawn / 2.0, short(*buy), t.bull, Align::Left, Baseline::Middle, size, None, Some(view.plot));
                    }
                }
                Mode::Cluster => {
                    // No numbers: the length of the bar is the reading, which
                    // is what a chart zoomed out has room to say.
                    let share = ((buy + sell) / loudest).clamp(0.0, 1.0) as f32;
                    f.rect(x - half, top, width * share, drawn, side.with_alpha(alpha(buy + sell)));
                }
                Mode::Delta | Mode::Total | Mode::Ladder => {
                    let value = match mode {
                        Mode::Total => buy + sell,
                        Mode::Ladder => running,
                        _ => buy - sell,
                    };
                    let color = if mode == Mode::Total { side } else if value >= 0.0 { t.bull } else { t.bear };
                    f.rect(x - half, top, width, drawn, color.with_alpha(alpha(value.abs())));
                    if numbers {
                        let text = if mode != Mode::Total && value >= 0.0 { format!("+{}", short(value)) } else { short(value) };
                        f.text(x, top + drawn / 2.0, text, t.text, Align::Center, Baseline::Middle, size, None, Some(view.plot));
                    }
                }
            }
        }

        // A side that took far more than the other side took one level away.
        // The diagonal, because a buyer lifts the offer where a seller hits the
        // bid a level beneath it.
        let mut bought_runs = vec![false; bar.levels.len()];
        let mut sold_runs = vec![false; bar.levels.len()];
        if numbers || row >= 2.0 {
            let at = |price: f64| bar.levels.iter().find(|(p, _, _)| (p - price).abs() < step / 2.0);
            for (i, (price, buy, sell)) in bar.levels.iter().enumerate() {
                let (top, drawn) = (top_of(*price), drawn_at(*price));
                let below_sell = at(*price - step).map_or(0.0, |(_, _, s)| *s);
                let above_buy = at(*price + step).map_or(0.0, |(_, b, _)| *b);
                let (bought_into, sold_into) = imbalanced(*buy, *sell, below_sell, above_buy, loudest);
                bought_runs[i] = bought_into;
                sold_runs[i] = sold_into;
                if !on_screen(*price) {
                    continue;
                }
                if bought_into {
                    f.stroke_rect(if mode == Mode::Split { x } else { x - half }, top, if mode == Mode::Split { half } else { width }, drawn, 1.0, t.bull);
                }
                if sold_into {
                    f.stroke_rect(x - half, top, if mode == Mode::Split { half } else { width }, drawn, 1.0, t.bear);
                }
            }
        }

        // Three imbalances in a row or more: a band of one-sided aggression,
        // which is what an order-flow reader is actually looking for. Marked
        // beside the column so the run reads as one thing.
        for (side, runs) in [(t.bull, stacked(&bought_runs)), (t.bear, stacked(&sold_runs))] {
            for (from, to) in runs {
                let top = top_of(bar.levels[to].0);
                let bottom = top_of(bar.levels[from].0) + drawn_at(bar.levels[from].0);
                let at_x = if side == t.bull { x + half + 1.0 } else { x - half - 3.0 };
                f.rect(at_x, top, 2.0, bottom - top, side);
            }
        }

        // The rows holding most of the bar's volume, and the one that held most.
        if options.value_area {
            if let Some((low, high)) = value_area(&bar.levels) {
                let top = top_of(bar.levels[high].0);
                let bottom = top_of(bar.levels[low].0) + drawn_at(bar.levels[low].0);
                f.stroke_rect(x - half, top, width, bottom - top, 1.0, t.text.with_alpha(70));
            }
        }
        if let Some(index) = bar.levels.iter().enumerate().max_by(|a, b| (a.1 .1 + a.1 .2).total_cmp(&(b.1 .1 + b.1 .2))).map(|(i, _)| i) {
            let price = bar.levels[index].0;
            f.stroke_rect(x - half, top_of(price), width, drawn_at(price), 1.0, t.text.with_alpha(150));
        }

        // What the whole bar came to, under it: the volume, and the delta with
        // the side that won it.
        if options.stats && numbers {
            let (bought, sold): (f64, f64) = bar.levels.iter().fold((0.0, 0.0), |(b, s), (_, buy, sell)| (b + buy, s + sell));
            let foot = top_of(bar.levels[0].0) + drawn_at(bar.levels[0].0) + 2.0;
            let delta = bought - sold;
            f.text(x, foot, short(bought + sold), t.text.with_alpha(170), Align::Center, Baseline::Top, size, None, Some(view.plot));
            let text = if delta >= 0.0 { format!("+{}", short(delta)) } else { short(delta) };
            f.text(x, foot + size + 2.0, text, if delta >= 0.0 { t.bull } else { t.bear }, Align::Center, Baseline::Top, size, None, Some(view.plot));
        }

        // The line between the two sides, so a column reads as one bar.
        if mode == Mode::Split {
            let top = top_of(bar.levels.last().map_or(0.0, |l| l.0));
            let bottom = top_of(bar.levels[0].0) + drawn_at(bar.levels[0].0);
            f.vline(f.snap(x), top, bottom, 1.0, t.border);
        }
    }

    // Every visible bar summed by price, against the right edge: where the
    // screen as a whole traded, which no single column can say.
    if options.profile {
        let mut rows: Vec<(f64, f64, f64)> = Vec::new();
        for (_, bar) in bars {
            for (price, buy, sell) in &bar.levels {
                match rows.iter_mut().find(|(p, _, _)| (p - price).abs() < grid.max(f64::EPSILON) / 2.0) {
                    Some(row) => {
                        row.1 += buy;
                        row.2 += sell;
                    }
                    None => rows.push((*price, *buy, *sell)),
                }
            }
        }
        let loudest = rows.iter().map(|(_, buy, sell)| buy + sell).fold(0.0_f64, f64::max);
        let step = if grid > 0.0 { grid } else { 1.0 };
        let room = view.plot.w * PROFILE_SHARE;
        for (price, buy, sell) in &rows {
            let (top, drawn) = ((view.y)(price + step), (row_height(view, *price, step) - 0.5).max(MIN_ROW));
            if top + drawn < view.plot.y || top > view.plot.bottom() || loudest <= 0.0 {
                continue;
            }
            let w = (room * ((buy + sell) / loudest) as f32).max(1.0);
            let color = if buy >= sell { t.bull } else { t.bear };
            f.rect(view.plot.right() - w, top, w, drawn, color.with_alpha(90));
        }
    }
}

/// The rows holding `VALUE_AREA` of a bar's volume, taken from the busiest
/// level outwards, a neighbour at a time — the way every platform builds it.
/// Returns the lowest and highest row of that band.
fn value_area(levels: &[(f64, f64, f64)]) -> Option<(usize, usize)> {
    let total: f64 = levels.iter().map(|(_, buy, sell)| buy + sell).sum();
    if total <= 0.0 {
        return None;
    }
    let at = |i: usize| levels[i].1 + levels[i].2;
    let poc = (0..levels.len()).max_by(|a, b| at(*a).total_cmp(&at(*b)))?;
    let (mut low, mut high, mut held) = (poc, poc, at(poc));
    while held < total * VALUE_AREA && (low > 0 || high + 1 < levels.len()) {
        let below = if low > 0 { at(low - 1) } else { f64::MIN };
        let above = if high + 1 < levels.len() { at(high + 1) } else { f64::MIN };
        if above >= below {
            high += 1;
            held += above;
        } else {
            low -= 1;
            held += below;
        }
    }
    Some((low, high))
}

/// Runs of `STACKED` or more imbalances on the same side, as index ranges.
fn stacked(flags: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, on) in flags.iter().chain(std::iter::once(&false)).enumerate() {
        match (*on, start) {
            (true, None) => start = Some(i),
            (false, Some(from)) => {
                if i - from >= STACKED {
                    runs.push((from, i - 1));
                }
                start = None;
            }
            _ => {}
        }
    }
    runs
}

/// Whether a side of a level took far more than the other side took one level
/// away — the diagonal, because a buyer lifts the offer where a seller hits the
/// bid beneath it. A level too quiet against the bar's loudest says nothing,
/// whatever its ratio: one coin against a tenth of one is not aggression.
fn imbalanced(buy: f64, sell: f64, below_sell: f64, above_buy: f64, loudest: f64) -> (bool, bool) {
    let loud = |v: f64| v >= loudest * IMBALANCE_FLOOR;
    let over = |side: f64, diagonal: f64| side >= diagonal * IMBALANCE && side > 0.0;
    (loud(buy) && over(buy, below_sell), loud(sell) && over(sell, above_buy))
}

/// How tall one level stands on screen, measured at the price it sits at.
fn row_height(view: &View<'_>, price: f64, step: f64) -> f32 {
    ((view.y)(price) - (view.y)(price + step)).abs().max(MIN_ROW)
}

/// The gap between levels, which is the market's own step wherever two levels
/// are neighbours — a bar that traded at four prices in a row says so itself.
pub(crate) fn level_step(bar: &Bar) -> f64 {
    let mut step = f64::MAX;
    for pair in bar.levels.windows(2) {
        let gap = pair[1].0 - pair[0].0;
        if gap > 0.0 && gap < step {
            step = gap;
        }
    }
    if step == f64::MAX {
        1.0
    } else {
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Bar {
        Bar { time: 0, levels: vec![(100.0, 1.0, 4.0), (101.0, 2.0, 1.0), (102.0, 8.0, 0.5)] }
    }

    #[test]
    fn a_number_is_short_enough_for_the_cell_it_sits_in() {
        assert_eq!(short(1_240.0), "1K");
        assert_eq!(short(142.0), "142");
        assert_eq!(short(4.25), "4.2");
        assert_eq!(short(0.37), "0.37");
        assert_eq!(short(0.004), "", "below a hundredth of a coin, the cell says nothing");
        assert_eq!(short(-3.0), "-3.0", "a delta that lost its minus would say the opposite");
    }

    #[test]
    fn the_step_between_levels_is_the_market_s_own() {
        assert!((level_step(&bar()) - 1.0).abs() < f64::EPSILON);
        assert!((level_step(&Bar { time: 0, levels: vec![(100.0, 1.0, 1.0), (100.5, 1.0, 1.0)] }) - 0.5).abs() < f64::EPSILON);
        assert!((level_step(&Bar { time: 0, levels: vec![(100.0, 1.0, 1.0)] }) - 1.0).abs() < f64::EPSILON, "one level alone has no gap to measure");
    }

    /// A cell is as tall as its own level is on screen — never as tall as the
    /// chart. Measuring the step from price zero said otherwise on a scale
    /// where zero is nowhere near, and painted a bar over the whole plot.
    #[test]
    fn a_cell_is_as_tall_as_its_level_even_where_zero_is_nowhere_near() {
        let theme = Theme::default();
        let plot = Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 };
        let mut f = Frame::default();
        f.begin(400.0, 300.0, 1.0, theme.background);
        let x = |l: f64| 200.0 + l as f32;
        // A logarithmic scale over prices around 84,000, as a chart of BTC has.
        let y = |p: f64| 300.0 - ((p.ln() - 84_000f64.ln()) * 100_000.0) as f32;
        let bar = Bar { time: 0, levels: vec![(84_000.0, 1.0, 2.0), (84_001.0, 3.0, 1.0), (84_002.0, 2.0, 2.0)] };
        let view = View { plot, bar_spacing: 30.0, char_width: 6.0, theme: &theme, x: &x, y: &y };
        paint(&mut f, &view, &[(0.0, &bar)], Options { mode: Mode::Split, value_area: false, stats: false, profile: false }, 1.0);
        // A dollar at 84,000 is about a pixel on this scale, so every vertex
        // sits within a few pixels of the levels — not spread over the plot.
        let ys: Vec<f32> = f.positions.chunks_exact(2).map(|p| p[1]).collect();
        let (top, bottom) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));
        assert!(bottom - top < 20.0, "the column stands where its levels are, not over the whole chart: {top} to {bottom}");
    }

    #[test]
    fn the_value_area_grows_from_the_busiest_level_outwards() {
        // A hundred coins: 3, 25, 50, 15, 7. The busiest row holds fifty, and
        // the seventy is reached by taking the busier neighbour, beneath it.
        let levels = vec![(100.0, 2.0, 1.0), (101.0, 13.0, 12.0), (102.0, 25.0, 25.0), (103.0, 8.0, 7.0), (104.0, 4.0, 3.0)];
        assert_eq!(value_area(&levels), Some((1, 2)), "the point of control and the busier side of it");
        assert_eq!(value_area(&[(100.0, 1.0, 1.0)]), Some((0, 0)), "one level is its own value area");
        assert_eq!(value_area(&[(100.0, 0.0, 0.0)]), None, "a bar that traded nothing has none");
    }

    #[test]
    fn three_imbalances_in_a_row_are_a_run_and_two_are_not() {
        assert_eq!(stacked(&[false, true, true, true, false]), vec![(1, 3)]);
        assert_eq!(stacked(&[true, true, false, true, true]), vec![], "two is not a stack");
        assert_eq!(stacked(&[true, true, true, true]), vec![(0, 3)], "and a run to the end is still a run");
        assert_eq!(stacked(&[]), vec![]);
    }

    #[test]
    fn a_side_that_took_far_more_than_the_diagonal_is_an_imbalance() {
        // Ten bought against one sold a level beneath: the buyers were paying up.
        assert_eq!(imbalanced(10.0, 1.0, 1.0, 1.0, 10.0), (true, false));
        // The same on the other side of the book.
        assert_eq!(imbalanced(1.0, 10.0, 1.0, 1.0, 10.0), (false, true));
        // Evenly matched, whichever way it is read.
        assert_eq!(imbalanced(1.0, 1.0, 1.0, 1.0, 10.0), (false, false));
        // A tenth of a coin against a hundredth is ten times as much and still
        // nothing at all, on a bar that traded ten.
        assert_eq!(imbalanced(0.1, 0.0, 0.01, 0.0, 10.0), (false, false));
        // Nothing on the diagonal at all, and a side worth noticing: an
        // imbalance, since the other side was not there to take it.
        assert_eq!(imbalanced(5.0, 0.0, 0.0, 0.0, 10.0), (true, false));
    }

    #[test]
    fn a_quiet_bar_is_as_legible_as_the_busy_one_beside_it() {
        let theme = Theme::default();
        let plot = Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 };
        let mut f = Frame::default();
        f.begin(400.0, 300.0, 1.0, theme.background);
        let x = |l: f64| 100.0 + l as f32 * 100.0;
        let y = |p: f64| 300.0 - (p as f32 - 99.0) * 100.0;
        let view = View { plot, bar_spacing: 60.0, char_width: 6.0, theme: &theme, x: &x, y: &y };
        // One bar trades in hundreds, the next in tenths.
        let busy = Bar { time: 0, levels: vec![(100.0, 400.0, 100.0), (101.0, 50.0, 200.0)] };
        let quiet = Bar { time: 60_000, levels: vec![(100.0, 0.4, 0.1), (101.0, 0.05, 0.2)] };
        paint(&mut f, &view, &[(0.0, &busy), (1.0, &quiet)], Options { mode: Mode::Split, value_area: false, stats: false, profile: false }, 1.0);
        // The strongest cell of each bar carries the same weight on screen.
        let alphas: Vec<u8> = f.colors.chunks_exact(4).map(|c| c[3]).collect();
        let strongest = *alphas.iter().max().unwrap();
        let faintest = *alphas.iter().min().unwrap();
        assert!(strongest > 240, "the loudest level of a bar is filled: {strongest}");
        assert!(faintest >= 55, "and the quietest is still visible, not a wash: {faintest}");
        assert_eq!(alphas.iter().filter(|a| **a > 240).count() % 6, 0, "each bar has its own loudest, filled the same");
    }

    #[test]
    fn each_mode_writes_what_it_is_about() {
        let theme = Theme::default();
        let plot = Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 };
        let wrote = |mode: Mode| {
            let mut f = Frame::default();
            f.begin(400.0, 300.0, 1.0, theme.background);
            let x = |l: f64| 200.0 + l as f32;
            let y = |p: f64| 300.0 - (p as f32 - 99.0) * 100.0;
            let view = View { plot, bar_spacing: 80.0, char_width: 6.0, theme: &theme, x: &x, y: &y };
            paint(&mut f, &view, &[(0.0, &bar())], Options { mode, value_area: false, stats: false, profile: false }, 1.0);
            f.texts.iter().map(|t| t.text.clone()).collect::<Vec<_>>()
        };
        // The lowest level was 1 bought against 4 sold.
        assert!(wrote(Mode::Split).contains(&"4.0".to_string()) && wrote(Mode::Split).contains(&"1.0".to_string()), "both sides of it");
        assert!(wrote(Mode::Delta).contains(&"-3.0".to_string()), "the difference, and which way it went");
        assert!(wrote(Mode::Total).contains(&"5.0".to_string()), "everything that traded there");
    }

    #[test]
    fn a_wide_bar_carries_its_numbers_and_a_narrow_one_only_its_cells() {
        let theme = Theme::default();
        let plot = Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 };
        let paint_at = |spacing: f32| {
            let mut f = Frame::default();
            f.begin(400.0, 300.0, 1.0, theme.background);
            let x = |l: f64| 200.0 + l as f32;
            // A hundred pixels to the price, so the rows are tall.
            let y = |p: f64| 300.0 - (p as f32 - 99.0) * 100.0;
            let view = View { plot, bar_spacing: spacing, char_width: 6.0, theme: &theme, x: &x, y: &y };
            paint(&mut f, &view, &[(0.0, &bar())], Options { mode: Mode::Split, value_area: false, stats: false, profile: false }, 1.0);
            f
        };
        let wide = paint_at(80.0);
        assert!(wide.texts.iter().any(|t| t.text == "8.0"), "the loudest level writes what it was");
        assert!(wide.texts.len() >= 6, "both sides of every level: {:?}", wide.texts.len());

        let narrow = paint_at(6.0);
        assert!(narrow.texts.is_empty(), "no room for numbers, so none are drawn");
        assert!(narrow.vertex_count() > 0, "the cells are still there");
    }
}
