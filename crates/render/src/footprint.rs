//! Footprint columns: what was bought and what was sold at every price inside
//! a bar, in place of the bar's body.
//!
//! A bar is a column of rows, one a price level, each row two cells — sold on
//! the left, bought on the right — filled in proportion to the busiest level in
//! view, so a glance shows where the trade actually happened. The numbers are
//! written only where a cell is big enough to hold them: sixty bars of forty
//! levels is five thousand numbers, which no chart draws sixty times a second.
//! Below that size the cells alone carry it, as a heat map does.

use crate::color::Color;
use crate::paint::{Align, Baseline, Frame, Rect};
use crate::theme::Theme;

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

/// How a volume is written inside a cell: short, because the room is short.
fn short(v: f64) -> String {
    match v.abs() {
        n if n >= 1_000.0 => format!("{:.0}K", n / 1_000.0),
        n if n >= 100.0 => format!("{n:.0}"),
        n if n >= 1.0 => format!("{n:.1}"),
        n if n >= 0.01 => format!("{n:.2}"),
        _ => String::new(),
    }
}

/// The bars in view, each at its logical index.
pub fn paint(f: &mut Frame, view: &View<'_>, bars: &[(f64, &Bar)]) {
    let t = view.theme;
    // Every cell is filled against the busiest level on screen, so one bar's
    // scale is another's and the eye can compare them.
    let loudest = bars
        .iter()
        .flat_map(|(_, bar)| bar.levels.iter())
        .map(|(_, buy, sell)| buy.max(*sell))
        .fold(0.0_f64, f64::max);
    if loudest <= 0.0 {
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
        // A row is as tall as the gap to the next level, so the column is solid.
        let step = level_step(bar);
        let row = ((view.y)(0.0) - (view.y)(step)).abs().max(MIN_ROW);
        let numbers = width >= NUMBER_WIDTH && row >= NUMBER_HEIGHT;
        for (price, buy, sell) in &bar.levels {
            let y = (view.y)(*price + step / 2.0);
            if y + row < view.plot.y || y > view.plot.bottom() {
                continue;
            }
            let h = (row - 0.5).max(MIN_ROW);
            let fill = |v: f64| (v / loudest).clamp(0.0, 1.0) as f32;
            f.rect(x - half, y, half, h, t.bear.with_alpha((30.0 + 190.0 * fill(*sell)) as u8));
            f.rect(x, y, half, h, t.bull.with_alpha((30.0 + 190.0 * fill(*buy)) as u8));
            if numbers {
                let pad = 3.0;
                f.text(x - pad, y + h / 2.0, short(*sell), t.text, Align::Right, Baseline::Middle, row.min(11.0) * 0.82, None, Some(view.plot));
                f.text(x + pad, y + h / 2.0, short(*buy), t.text, Align::Left, Baseline::Middle, row.min(11.0) * 0.82, None, Some(view.plot));
            }
        }
        // The line between the two sides, so a column reads as one bar.
        let top = (view.y)(bar.levels.last().map_or(0.0, |l| l.0) + step);
        let bottom = (view.y)(bar.levels.first().map_or(0.0, |l| l.0));
        f.vline(f.snap(x), top, bottom, 1.0, t.border);
    }
}

/// The gap between levels, which is the market's own step wherever two levels
/// are neighbours — a bar that traded at four prices in a row says so itself.
fn level_step(bar: &Bar) -> f64 {
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
    }

    #[test]
    fn the_step_between_levels_is_the_market_s_own() {
        assert!((level_step(&bar()) - 1.0).abs() < f64::EPSILON);
        assert!((level_step(&Bar { time: 0, levels: vec![(100.0, 1.0, 1.0), (100.5, 1.0, 1.0)] }) - 0.5).abs() < f64::EPSILON);
        assert!((level_step(&Bar { time: 0, levels: vec![(100.0, 1.0, 1.0)] }) - 1.0).abs() < f64::EPSILON, "one level alone has no gap to measure");
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
            paint(&mut f, &view, &[(0.0, &bar())]);
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
