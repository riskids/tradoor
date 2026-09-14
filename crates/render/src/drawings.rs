//! Drawing tools: what each kind is, and how it is painted.

use tradoor_core::format::format_price;

use crate::color::Color;
use crate::paint::{Align, Baseline, Frame, Rect};
use crate::theme::Theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrawingKind {
    Path,
    Brush,
    Trendline,
    Horizontal,
    Ray,
    Rect,
    Fib,
    Text,
    Measure,
    Long,
    Short,
}

impl DrawingKind {
    pub const ALL: [DrawingKind; 11] = [Self::Path, Self::Brush, Self::Trendline, Self::Horizontal, Self::Ray, Self::Rect, Self::Fib, Self::Text, Self::Measure, Self::Long, Self::Short];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Brush => "brush",
            Self::Trendline => "trendline",
            Self::Horizontal => "horizontal",
            Self::Ray => "ray",
            Self::Rect => "rect",
            Self::Fib => "fib",
            Self::Text => "text",
            Self::Measure => "measure",
            Self::Long => "long",
            Self::Short => "short",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Anchors the kind takes — the minimum for a path or brush.
    pub fn arity(self) -> usize {
        match self {
            Self::Horizontal | Self::Ray | Self::Text => 1,
            Self::Long | Self::Short => 3,
            _ => 2,
        }
    }

    pub fn open_ended(self) -> bool {
        matches!(self, Self::Path | Self::Brush)
    }
}

pub const FIB_LEVELS: [f64; 7] = [0.0, 0.236, 0.382, 0.5, 0.618, 0.786, 1.0];
pub const MAX_POINTS: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    /// Epoch ms, on the candles' time axis.
    pub time: f64,
    pub price: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawingShape {
    /// `None` while it is still being placed.
    pub id: Option<i64>,
    pub kind: DrawingKind,
    pub points: Vec<Anchor>,
    pub label: Option<String>,
}

/// A drawing's own numbers use significant digits rather than the instrument's
/// precision: a measure's delta is derived, and seven decimals of it help nobody.
pub fn significant(value: f64) -> String {
    let abs = value.abs();
    if abs >= 1000.0 {
        format!("{value:.0}")
    } else if abs >= 1.0 {
        format!("{value:.2}")
    } else if abs == 0.0 {
        "0".into()
    } else {
        let decimals = (2 - abs.log10().floor() as i32).max(0) as usize;
        format!("{value:.decimals$}")
    }
}

pub struct Placed<'a> {
    pub shape: &'a DrawingShape,
    pub points: Vec<(f32, f32)>,
    pub selected: bool,
    pub draft: bool,
    /// Bars between the first two anchors, for the measure readout.
    pub bars: i64,
}

fn line(frame: &mut Frame, a: (f32, f32), b: (f32, f32), width: f32, color: Color, dashed: bool) {
    if dashed {
        frame.dashed_line(a.0, a.1, b.0, b.1, width, color, 4.0, 3.0);
    } else if a.1 == b.1 {
        frame.hline(a.0, b.0, a.1, width, color);
    } else {
        frame.line(a.0, a.1, b.0, b.1, width, color);
    }
}

pub fn paint(frame: &mut Frame, d: &Placed<'_>, plot: Rect, theme: &Theme) {
    let Some(&a) = d.points.first() else { return };
    let color = if d.selected { theme.drawing_selected } else { theme.drawing };
    let width = if d.selected { 2.0 } else { 1.5 };
    let prices: Vec<f64> = d.shape.points.iter().map(|p| p.price).collect();
    let font = theme.font_px - 1.0;
    let label = |frame: &mut Frame, x: f32, y: f32, text: String| frame.text(x, y, text, color, Align::Left, Baseline::Bottom, font, None, Some(plot));
    let (b, c) = (d.points.get(1).copied(), d.points.get(2).copied());

    match (d.shape.kind, b, c) {
        (crate::DrawingKind::Path | crate::DrawingKind::Brush, _, _) => {
            let w = if d.shape.kind == crate::DrawingKind::Brush { width + 0.5 } else { width };
            if d.draft {
                for pair in d.points.windows(2) {
                    line(frame, pair[0], pair[1], w, color, true);
                }
            } else {
                frame.polyline(&d.points, w, color);
            }
        }
        (crate::DrawingKind::Horizontal, _, _) => line(frame, (plot.x, a.1), (plot.right(), a.1), width, color, d.draft),
        (crate::DrawingKind::Ray, _, _) => line(frame, a, (plot.right(), a.1), width, color, d.draft),
        (crate::DrawingKind::Text, _, _) => {
            frame.vline(a.0, a.1 - 4.0, a.1 + 4.0, width, color);
            label(frame, a.0 + 6.0, a.1 - 2.0, d.shape.label.clone().unwrap_or_default());
        }
        (crate::DrawingKind::Rect, Some(b), _) => {
            frame.rect(a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs(), theme.drawing_fill);
            frame.stroke_rect(a.0, a.1, b.0 - a.0, b.1 - a.1, width, color);
        }
        (crate::DrawingKind::Fib, Some(b), _) => {
            let left = a.0.min(b.0);
            for level in FIB_LEVELS {
                let y = a.1 + (b.1 - a.1) * level as f32;
                let price = prices[0] + (prices[1] - prices[0]) * level;
                let c = if level == 0.0 || level == 1.0 { color } else { color.with_alpha(166) };
                line(frame, (left, y), (plot.right(), y), width, c, d.draft);
                label(frame, left + 4.0, y - 2.0, format!("{:.1}%  {}", level * 100.0, significant(price)));
            }
        }
        (crate::DrawingKind::Measure, Some(b), _) => {
            frame.rect(a.0.min(b.0), a.1.min(b.1), (b.0 - a.0).abs(), (b.1 - a.1).abs(), theme.drawing_fill);
            frame.stroke_rect(a.0, a.1, b.0 - a.0, b.1 - a.1, width, color);
            let mid = (a.1 + b.1) / 2.0;
            line(frame, (a.0, mid), (b.0, mid), width, color, false);
            let dir = if b.0 >= a.0 { 1.0 } else { -1.0 };
            frame.line(b.0, mid, b.0 - 5.0 * dir, mid - 3.0, width, color);
            frame.line(b.0, mid, b.0 - 5.0 * dir, mid + 3.0, width, color);
            let (from, to) = (prices[0], prices[1]);
            let pct = if from == 0.0 { 0.0 } else { (to - from) / from * 100.0 };
            let sign = if to >= from { "+" } else { "" };
            let bars = d.bars.abs();
            label(frame, a.0.min(b.0) + 4.0, a.1.min(b.1) - 3.0, format!("{sign}{}  {sign}{pct:.2}%  {bars} bar{}", significant(to - from), if bars == 1 { "" } else { "s" }));
        }
        (crate::DrawingKind::Long | crate::DrawingKind::Short, Some(b), Some(c)) => {
            let left = a.0.min(b.0).min(c.0);
            let w = (a.0.max(b.0).max(c.0) - left).max(40.0);
            frame.rect(left, a.1.min(b.1), w, (b.1 - a.1).abs(), theme.risk_fill);
            frame.stroke_rect(left, a.1, w, b.1 - a.1, 1.0, theme.risk);
            frame.rect(left, a.1.min(c.1), w, (c.1 - a.1).abs(), theme.reward_fill);
            frame.stroke_rect(left, a.1, w, c.1 - a.1, 1.0, theme.reward);
            let (entry, stop, target) = (prices[0], prices[1], prices.get(2).copied().unwrap_or(prices[0]));
            let risk = (entry - stop).abs();
            let rr = if risk == 0.0 { 0.0 } else { (target - entry).abs() / risk };
            let side = if d.shape.kind == crate::DrawingKind::Long { "LONG" } else { "SHORT" };
            label(frame, left + 4.0, a.1.min(b.1).min(c.1) - 3.0, format!("{side}  R:R {rr:.2}  risk {}", significant(risk)));
        }
        (_, Some(b), _) => line(frame, a, b, width, color, d.draft),
        _ => {}
    }

    if d.selected && d.shape.kind != crate::DrawingKind::Brush {
        for &p in &d.points {
            frame.circle(p.0, p.1, 4.5, color);
            frame.circle(p.0, p.1, 3.0, theme.handle);
        }
    }
    let _ = format_price;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_and_significant_digits() {
        for k in DrawingKind::ALL {
            assert_eq!(DrawingKind::parse(k.as_str()), Some(k));
            assert!(k.arity() >= 1);
        }
        assert_eq!(significant(77_123.4), "77123");
        assert_eq!(significant(12.345), "12.35");
        assert_eq!(significant(0.000012345), "0.0000123");
        assert_eq!(significant(-0.5), "-0.500");
    }
}
