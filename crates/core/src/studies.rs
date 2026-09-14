//! Indicators a reader adds to the chart.
//!
//! Studies sharing a `group` share a pane: a pane has one price scale, and mixing
//! scales is how RSI ends up a flat line beside a MACD measured in dollars.

use crate::indicators::{self, Series};
use crate::time::{weekday, week_index, month_index, year_of, DAY, HOUR};
use crate::Candle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StudyKind {
    Ema,
    Sma,
    Bb,
    Rsi,
    StochRsi,
    Macd,
    Atr,
    KeyLevels,
    Avwap,
    SwingMap,
}

impl StudyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ema => "ema",
            Self::Sma => "sma",
            Self::Bb => "bb",
            Self::Rsi => "rsi",
            Self::StochRsi => "stochrsi",
            Self::Macd => "macd",
            Self::Atr => "atr",
            Self::KeyLevels => "keylevels",
            Self::Avwap => "avwap",
            Self::SwingMap => "swingmap",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Price,
    Pct,
    Macd,
    Atr,
}

impl Group {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Price => "price",
            Self::Pct => "pct",
            Self::Macd => "macd",
            Self::Atr => "atr",
        }
    }
}

pub struct StudyParam {
    pub name: &'static str,
    pub default: u32,
    pub min: u32,
    pub max: u32,
}

pub struct StudyDefinition {
    pub kind: StudyKind,
    pub name: &'static str,
    pub short: &'static str,
    pub group: Group,
    pub params: &'static [StudyParam],
    pub about: &'static str,
}

const fn p(name: &'static str, default: u32, min: u32, max: u32) -> StudyParam {
    StudyParam { name, default, min, max }
}

pub static STUDIES: [StudyDefinition; 10] = [
    StudyDefinition { kind: StudyKind::Ema, name: "Exponential Moving Average", short: "EMA", group: Group::Price, params: &[p("length", 50, 2, 1000)], about: "Average close, weighted towards recent bars." },
    StudyDefinition { kind: StudyKind::Sma, name: "Simple Moving Average", short: "SMA", group: Group::Price, params: &[p("length", 50, 2, 1000)], about: "Plain average of the last N closes — every bar in the window counts the same." },
    StudyDefinition { kind: StudyKind::Bb, name: "Bollinger Bands", short: "BB", group: Group::Price, params: &[p("length", 20, 2, 1000), p("mult", 2, 1, 10)], about: "An SMA with a band at N standard deviations. Narrow bands mark the quiet before an expansion." },
    StudyDefinition { kind: StudyKind::Rsi, name: "Relative Strength Index", short: "RSI", group: Group::Pct, params: &[p("length", 14, 2, 1000)], about: "Momentum bounded 0–100. Wilder smoothing, so the values match TradingView exactly." },
    StudyDefinition { kind: StudyKind::StochRsi, name: "Stochastic RSI", short: "Stoch RSI", group: Group::Pct, params: &[p("rsi", 14, 2, 1000), p("stoch", 14, 2, 1000), p("k", 3, 1, 100), p("d", 3, 1, 100)], about: "Where RSI sits inside its own recent range. Turns earlier than RSI, and whipsaws more." },
    StudyDefinition { kind: StudyKind::Macd, name: "MACD", short: "MACD", group: Group::Macd, params: &[p("fast", 12, 1, 1000), p("slow", 26, 2, 1000), p("signal", 9, 1, 1000)], about: "The gap between two EMAs, and an EMA of that gap. The histogram is the difference between them." },
    StudyDefinition { kind: StudyKind::KeyLevels, name: "Historical Key Levels", short: "Key Levels", group: Group::Price, params: &[], about: "Weekly, monthly and yearly opens plus Monday’s high and low — the prices the week is measured against." },
    StudyDefinition { kind: StudyKind::Avwap, name: "Anchored VWAP (S/W/M/Y)", short: "Multi-VWAP", group: Group::Price, params: &[], about: "Volume-weighted average price since the session, week, month and year opened. The session line is drawn on intraday charts only." },
    StudyDefinition { kind: StudyKind::SwingMap, name: "Range Swing Structure Map", short: "Swings", group: Group::Price, params: &[p("left", 5, 1, 100), p("right", 5, 1, 100)], about: "Zigzag of confirmed pivots tagged HH/HL/LH/LL, with each leg’s move and duration, and the averages across the window." },
    StudyDefinition { kind: StudyKind::Atr, name: "Average True Range", short: "ATR", group: Group::Atr, params: &[p("length", 14, 2, 1000)], about: "Average range of a bar, in the instrument’s own price units. A stop distance, not a direction." },
];

pub fn definition(kind: &str) -> Option<&'static StudyDefinition> {
    STUDIES.iter().find(|d| d.kind.as_str() == kind)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudySpec {
    pub kind: StudyKind,
    pub params: Vec<u32>,
}

impl StudySpec {
    fn def(&self) -> &'static StudyDefinition {
        STUDIES.iter().find(|d| d.kind == self.kind).expect("every kind has a definition")
    }

    /// `ema:50` — the id AND the wire format: what localStorage holds and what a
    /// legend row is keyed by, so a study cannot be added twice under two names.
    pub fn id(&self) -> String {
        std::iter::once(self.kind.as_str().to_string()).chain(self.params.iter().map(u32::to_string)).collect::<Vec<_>>().join(":")
    }

    /// `EMA 50`, `Stoch RSI 14 14 3 3`.
    pub fn label(&self) -> String {
        let short = self.def().short;
        if self.params.is_empty() {
            short.to_string()
        } else {
            format!("{short} {}", self.params.iter().map(u32::to_string).collect::<Vec<_>>().join(" "))
        }
    }

    pub fn group(&self) -> Group {
        self.def().group
    }
}

/// Parse `ema:50,rsi:14`. Unknown kinds and out-of-range parameters are DROPPED
/// rather than failing the list: it comes from storage an older version may have
/// written, and a chart that refuses to load is worse than one missing a study.
pub fn parse_studies(raw: &str, max: usize) -> Vec<StudySpec> {
    let mut out: Vec<StudySpec> = Vec::new();
    for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        let mut parts = token.split(':');
        let Some(def) = parts.next().and_then(definition) else { continue };
        let rest: Vec<&str> = parts.collect();
        let params: Option<Vec<u32>> = def
            .params
            .iter()
            .enumerate()
            .map(|(i, param)| {
                let value = match rest.get(i) {
                    None => f64::from(param.default),
                    Some(s) if s.trim().is_empty() => 0.0,
                    Some(s) => s.trim().parse::<f64>().ok()?,
                };
                (value.is_finite() && value >= f64::from(param.min) && value <= f64::from(param.max)).then(|| value.round() as u32)
            })
            .collect();
        let Some(params) = params else { continue };
        let spec = StudySpec { kind: def.kind, params };
        if out.iter().any(|s| s == &spec) {
            continue;
        }
        out.push(spec);
        if out.len() >= max {
            break;
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineType {
    Line,
    Histogram,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    /// Holds each value flat until the next — what an opening price does.
    Step,
    /// Not stroked: the series only carries values for the legend and the axis.
    None,
}

#[derive(Clone, Debug)]
pub struct StudyLine {
    pub key: &'static str,
    pub label: String,
    pub kind: LineType,
    pub values: Series,
    pub style: LineStyle,
    /// Fixed colour for lines told apart BY colour; otherwise the chart's palette.
    pub color: Option<&'static str>,
    pub width: f32,
}

impl StudyLine {
    fn line(key: &'static str, label: impl Into<String>, values: Series) -> Self {
        Self { key, label: label.into(), kind: LineType::Line, values, style: LineStyle::Solid, color: None, width: 1.0 }
    }
}

/// A level that holds for a bounded stretch of time — last week's open is a fact
/// about last week, not a line carried to the right edge.
#[derive(Clone, Debug, PartialEq)]
pub struct StudySegment {
    pub from: i64,
    pub to: i64,
    pub price: f64,
    pub label: &'static str,
    pub color: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Bull,
    Bear,
    Neutral,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudyMarker {
    pub time: i64,
    pub above: bool,
    pub text: String,
    pub tone: Tone,
}

#[derive(Clone, Debug)]
pub struct ComputedStudy {
    pub id: String,
    pub kind: StudyKind,
    pub label: String,
    pub group: Group,
    pub lines: Vec<StudyLine>,
    pub segments: Vec<StudySegment>,
    pub markers: Vec<StudyMarker>,
    pub stats: Vec<(&'static str, String)>,
}

pub fn compute_study(spec: &StudySpec, candles: &[Candle]) -> ComputedStudy {
    let def = spec.def();
    let close: Series = candles.iter().map(|c| Some(c.close)).collect();
    let label = spec.label();
    let param = |i: usize| spec.params.get(i).copied().unwrap_or(def.params.get(i).map_or(0, |p| p.default)) as usize;
    let mut study = ComputedStudy { id: spec.id(), kind: def.kind, label: label.clone(), group: def.group, lines: Vec::new(), segments: Vec::new(), markers: Vec::new(), stats: Vec::new() };

    match def.kind {
        StudyKind::Ema => study.lines.push(StudyLine::line("ema", label, indicators::ema(&close, param(0)))),
        StudyKind::Sma => study.lines.push(StudyLine::line("sma", label, indicators::sma(&close, param(0)))),
        StudyKind::Bb => {
            let b = indicators::bollinger(&close, param(0), param(1) as f64);
            study.lines = vec![StudyLine::line("upper", "Upper", b.upper), StudyLine::line("basis", "Basis", b.basis), StudyLine::line("lower", "Lower", b.lower)];
        }
        StudyKind::Rsi => study.lines.push(StudyLine::line("rsi", label, indicators::rsi(&close, param(0)))),
        StudyKind::StochRsi => {
            let s = indicators::stoch_rsi(&close, param(0), param(1), param(2), param(3));
            study.lines = vec![StudyLine::line("k", "%K", s.k), StudyLine::line("d", "%D", s.d)];
        }
        StudyKind::Macd => {
            let m = indicators::macd(&close, param(0), param(1), param(2));
            // Histogram first so the lines are drawn over the bars.
            study.lines = vec![
                StudyLine { kind: LineType::Histogram, ..StudyLine::line("histogram", "Histogram", m.histogram) },
                StudyLine::line("macd", "MACD", m.macd),
                StudyLine::line("signal", "Signal", m.signal),
            ];
        }
        StudyKind::Atr => study.lines.push(StudyLine::line("atr", label, indicators::atr(candles, param(0)))),
        StudyKind::KeyLevels => (study.lines, study.segments) = key_levels(candles),
        StudyKind::Avwap => study.lines = anchored_vwap(candles),
        StudyKind::SwingMap => (study.lines, study.markers, study.stats) = swing_map(candles, param(0), param(1)),
    }
    study
}

const WEEKLY: &str = "#fffcbc";
const MONTHLY: &str = "#08d48c";
const YEARLY: &str = "#ff9800";
const MONDAY: &str = "#cfcfcf";

/// `2d 4h`, `6h`, `3d` — to the hour.
pub fn format_duration(ms: f64) -> String {
    let total_hours = (ms / HOUR as f64).floor() as i64;
    let (days, hours) = (total_hours.div_euclid(24), total_hours.rem_euclid(24));
    match (days > 0, hours > 0) {
        (true, true) => format!("{days}d {hours}h"),
        (true, false) => format!("{days}d"),
        _ => format!("{hours}h"),
    }
}

/// Weekly / monthly / yearly opens and Monday's range ("Boyen_Clean Historical
/// Key Levels"). The FIRST period in view is partial — its real open is before
/// the data — so a run only starts once a boundary turns over inside the data.
fn key_levels(candles: &[Candle]) -> (Vec<StudyLine>, Vec<StudySegment>) {
    #[derive(Clone, Copy)]
    struct Run {
        price: f64,
        from: i64,
        to: i64,
    }
    #[derive(Clone, Copy)]
    struct Monday {
        high: f64,
        low: f64,
        from: i64,
        to: i64,
    }

    let n = candles.len();
    let (mut wo, mut mo, mut yo, mut mon_high, mut mon_low) = (vec![None; n], vec![None; n], vec![None; n], vec![None; n], vec![None; n]);
    let mut segments = Vec::new();
    let (mut week_run, mut month_run, mut year_run): (Option<Run>, Option<Run>, Option<Run>) = (None, None, None);
    let mut monday: Option<Monday> = None;
    let (mut week, mut month, mut year): (Option<i64>, Option<i64>, Option<i64>) = (None, None, None);

    let close = |segments: &mut Vec<StudySegment>, run: Option<Run>, label: &'static str, color: &'static str| {
        if let Some(r) = run {
            segments.push(StudySegment { from: r.from, to: r.to, price: r.price, label, color });
        }
    };

    for (i, c) in candles.iter().enumerate() {
        let (w, m, y) = (week_index(c.open_time), month_index(c.open_time), year_of(c.open_time));
        let start = Run { price: c.open, from: c.open_time, to: c.open_time };

        if week.is_some_and(|prev| prev != w) {
            close(&mut segments, week_run, "WO", WEEKLY);
            week_run = Some(start);
            monday = None;
        }
        week = Some(w);
        if month.is_some_and(|prev| prev != m) {
            close(&mut segments, month_run, "MO", MONTHLY);
            month_run = Some(start);
        }
        month = Some(m);
        if year.is_some_and(|prev| prev != y) {
            close(&mut segments, year_run, "YO", YEARLY);
            year_run = Some(start);
        }
        year = Some(y);

        for run in [&mut week_run, &mut month_run, &mut year_run].into_iter().flatten() {
            run.to = c.open_time;
        }
        if weekday(c.open_time) == 1 {
            match monday.as_mut() {
                None => monday = Some(Monday { high: c.high, low: c.low, from: c.open_time, to: c.open_time }),
                Some(mon) => {
                    mon.high = mon.high.max(c.high);
                    mon.low = mon.low.min(c.low);
                }
            }
        }
        if let Some(mon) = monday.as_mut() {
            mon.to = c.open_time;
        }
        wo[i] = week_run.map(|r| r.price);
        mo[i] = month_run.map(|r| r.price);
        yo[i] = year_run.map(|r| r.price);
        mon_high[i] = monday.map(|m| m.high);
        mon_low[i] = monday.map(|m| m.low);
    }
    close(&mut segments, week_run, "WO", WEEKLY);
    close(&mut segments, month_run, "MO", MONTHLY);
    close(&mut segments, year_run, "YO", YEARLY);
    // Only the current week's Monday is still a reference.
    if let Some(mon) = monday {
        segments.push(StudySegment { from: mon.from, to: mon.to, price: mon.high, label: "Mon H", color: MONDAY });
        segments.push(StudySegment { from: mon.from, to: mon.to, price: mon.low, label: "Mon L", color: MONDAY });
    }

    let hidden = |key: &'static str, label: &'static str, values: Series, color: &'static str| StudyLine { style: LineStyle::None, color: Some(color), ..StudyLine::line(key, label, values) };
    (
        vec![hidden("wo", "WO", wo, WEEKLY), hidden("mo", "MO", mo, MONTHLY), hidden("yo", "YO", yo, YEARLY), hidden("monH", "Mon H", mon_high, MONDAY), hidden("monL", "Mon L", mon_low, MONDAY)],
        segments,
    )
}

/// VWAP of hlc3 since the session, week, month and year opened ("Boyen_Multiple
/// Anchor VWAP"). The session line is empty unless bars are intraday — measured
/// from the data, so it stays right for folded timeframes no label covers.
fn anchored_vwap(candles: &[Candle]) -> Vec<StudyLine> {
    let n = candles.len();
    let (mut session, mut weekly, mut monthly, mut yearly) = (vec![None; n], vec![None; n], vec![None; n], vec![None; n]);
    let intraday = n >= 2 && candles[1].open_time - candles[0].open_time < DAY;
    let mut keys: [Option<i64>; 4] = [None; 4];
    let mut acc = [(0.0f64, 0.0f64); 4];

    for (i, c) in candles.iter().enumerate() {
        let typical = (c.high + c.low + c.close) / 3.0;
        let now = [c.open_time.div_euclid(DAY), week_index(c.open_time), month_index(c.open_time), year_of(c.open_time)];
        for k in 0..4 {
            if keys[k] != Some(now[k]) {
                keys[k] = Some(now[k]);
                acc[k] = (0.0, 0.0);
            }
            acc[k].0 += typical * c.volume;
            acc[k].1 += c.volume;
        }
        // A period with no volume has no VWAP — never the last price.
        let vwap = |k: usize| (acc[k].1 > 0.0).then(|| acc[k].0 / acc[k].1);
        if intraday {
            session[i] = vwap(0);
        }
        weekly[i] = vwap(1);
        monthly[i] = vwap(2);
        yearly[i] = vwap(3);
    }
    let colored = |key, label, values, color, width| StudyLine { color: Some(color), width, ..StudyLine::line(key, label, values) };
    vec![
        colored("session", "Session", session, "#2962ff", 1.0),
        colored("weekly", "Weekly", weekly, "#ffa726", 2.0),
        colored("monthly", "Monthly", monthly, "#e040fb", 2.0),
        colored("yearly", "Yearly", yearly, "#e8e8e8", 2.0),
    ]
}

/// Zigzag of CONFIRMED pivots tagged HH/HL/LH/LL ("Boyen_Range Swing Structure
/// Map"). The original's live, unconfirmed preview is not ported: a tentative
/// swing is a repaint, so the last leg ends `right` bars back.
fn swing_map(candles: &[Candle], left: usize, right: usize) -> (Vec<StudyLine>, Vec<StudyMarker>, Vec<(&'static str, String)>) {
    #[derive(Clone, Copy)]
    struct Point {
        index: usize,
        price: f64,
        high: bool,
        time: i64,
    }
    let n = candles.len();
    let highs: Series = candles.iter().map(|c| Some(c.high)).collect();
    let lows: Series = candles.iter().map(|c| Some(c.low)).collect();
    let mut pivots: Vec<Point> = indicators::pivot_highs(&highs, left, right)
        .into_iter()
        .map(|p| Point { index: p.index, price: p.value, high: true, time: candles[p.index].open_time })
        .chain(indicators::pivot_lows(&lows, left, right).into_iter().map(|p| Point { index: p.index, price: p.value, high: false, time: candles[p.index].open_time }))
        .collect();
    pivots.sort_by_key(|p| p.index);

    // Two of the same type in a row are one swing seen twice: keep the extreme.
    let mut points: Vec<Point> = Vec::new();
    for p in pivots {
        match points.last_mut() {
            Some(last) if last.high == p.high => {
                if if p.high { p.price > last.price } else { p.price < last.price } {
                    *last = p;
                }
            }
            _ => points.push(p),
        }
    }

    let mut zigzag = vec![None; n];
    let mut markers = Vec::new();
    let (mut up_pct, mut up_dur, mut dn_pct, mut dn_dur) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (i, p) in points.iter().enumerate() {
        zigzag[p.index] = Some(p.price);
        let mut text = points[..i]
            .iter()
            .rev()
            .find(|prev| prev.high == p.high)
            .map_or(if p.high { "H" } else { "L" }, |prev| match (p.high, p.price > prev.price) {
                (true, true) => "HH",
                (true, false) => "LH",
                (false, true) => "HL",
                (false, false) => "LL",
            })
            .to_string();
        if i > 0 {
            let prev = points[i - 1];
            let pct = (p.price - prev.price) / prev.price * 100.0;
            let dur = (p.time - prev.time) as f64;
            text += &format!(" {}{pct:.2}% {}", if pct >= 0.0 { "+" } else { "" }, format_duration(dur));
            if p.high {
                up_pct.push(pct);
                up_dur.push(dur);
            } else {
                dn_pct.push(pct);
                dn_dur.push(dur);
            }
            let span = p.index - prev.index;
            for k in 1..span {
                zigzag[prev.index + k] = Some(prev.price + (p.price - prev.price) * k as f64 / span as f64);
            }
        }
        markers.push(StudyMarker { time: p.time, above: p.high, text, tone: if p.high { Tone::Bear } else { Tone::Bull } });
    }

    let avg = |xs: &[f64]| (!xs.is_empty()).then(|| xs.iter().sum::<f64>() / xs.len() as f64);
    let na = || "n/a".to_string();
    let stats = vec![
        ("Swings", points.len().to_string()),
        ("Avg rise", avg(&up_pct).map_or_else(na, |v| format!("+{v:.2}%"))),
        ("Avg rise time", avg(&up_dur).map_or_else(na, format_duration)),
        ("Avg drop", avg(&dn_pct).map_or_else(na, |v| format!("{v:.2}%"))),
        ("Avg drop time", avg(&dn_dur).map_or_else(na, format_duration)),
    ];
    let line = StudyLine { color: Some("#8ab4f8"), width: 2.0, ..StudyLine::line("zigzag", "Swings", zigzag) };
    (vec![line], markers, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::parse_iso;

    fn ids(raw: &str) -> Vec<String> {
        parse_studies(raw, 12).iter().map(StudySpec::id).collect()
    }

    fn series(start: &str, count: usize, step: i64, shape: impl Fn(usize) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<f64>)) -> Vec<Candle> {
        let t0 = parse_iso(start).unwrap();
        (0..count)
            .map(|i| {
                let base = 100.0 + i as f64;
                let (o, h, l, c, v) = shape(i);
                Candle { open_time: t0 + i as i64 * step, open: o.unwrap_or(base), high: h.unwrap_or(base + 1.0), low: l.unwrap_or(base - 1.0), close: c.unwrap_or(base), volume: v.unwrap_or(1.0), quote_volume: None }
            })
            .collect()
    }

    fn line<'a>(study: &'a ComputedStudy, key: &str) -> &'a Series {
        &study.lines.iter().find(|l| l.key == key).unwrap_or_else(|| panic!("missing line {key}")).values
    }

    #[test]
    fn parsing_is_forgiving_and_exact() {
        assert_eq!(ids("ema:50,notathing:3,rsi:14"), ["ema:50", "rsi:14"]);
        assert_eq!(ids("macd"), ["macd:12:26:9"]);
        assert_eq!(ids("ema:1,ema:50"), ["ema:50"]);
        assert_eq!(ids("rsi:14,rsi:14").len(), 1);
        assert_eq!(parse_studies("stochrsi", 12)[0].label(), "Stoch RSI 14 14 3 3");
        assert_eq!(parse_studies("keylevels", 12)[0].label(), "Key Levels");
    }

    #[test]
    fn a_study_is_index_aligned_and_macd_draws_histogram_first() {
        let candles = series("2026-08-01", 40, DAY, |_| (None, None, None, None, None));
        let out = compute_study(&parse_studies("ema:10", 12)[0], &candles);
        assert_eq!((out.id.as_str(), out.group), ("ema:10", Group::Price));
        assert_eq!(out.lines[0].values.len(), 40);
        assert_eq!(out.lines[0].values[0], None);
        assert!(out.lines[0].values[39].is_some());

        let candles = series("2026-08-01", 61, DAY, |i| (None, None, None, Some(100.0 + (i as f64).sin() * 5.0), None));
        let out = compute_study(&parse_studies("macd", 12)[0], &candles);
        assert_eq!(out.lines.iter().map(|l| l.key).collect::<Vec<_>>(), ["histogram", "macd", "signal"]);
        assert_eq!(out.lines[0].kind, LineType::Histogram);
    }

    #[test]
    fn key_levels_skip_the_partial_first_period() {
        let candles = series("2026-08-03", 21, DAY, |_| (None, None, None, None, None));
        let study = compute_study(&parse_studies("keylevels", 12)[0], &candles);
        let wo = line(&study, "wo");
        assert!(wo[..7].iter().all(Option::is_none));
        assert_eq!(wo[7], Some(candles[7].open));
        assert!(wo[7..14].iter().all(|v| *v == Some(candles[7].open)));
        assert_ne!(wo[14], wo[13]);

        let weekly: Vec<_> = study.segments.iter().filter(|s| s.label == "WO").collect();
        assert!(weekly.len() >= 2);
        for w in weekly.windows(2) {
            assert!(w[1].from > w[0].to - 1);
        }
        assert!(weekly.iter().all(|s| s.to > s.from && s.to - s.from <= 7 * DAY));
        assert!(study.lines.iter().all(|l| l.style == LineStyle::None));
        let mondays: Vec<_> = study.segments.iter().filter(|s| s.label.starts_with("Mon")).collect();
        assert_eq!(mondays.len(), 2);
        assert_eq!(mondays[0].to, candles[20].open_time);

        let spiky = series("2026-08-03", 14, DAY, |i| if i == 1 { (None, Some(9999.0), Some(1.0), None, None) } else { (None, None, None, None, None) });
        let s = compute_study(&parse_studies("keylevels", 12)[0], &spiky);
        let mon_h = line(&s, "monH");
        assert_eq!((mon_h[0], mon_h[1], mon_h[7]), (Some(spiky[0].high), Some(spiky[0].high), Some(spiky[7].high)));
    }

    #[test]
    fn anchored_vwap_weights_and_resets() {
        let flat = series("2026-08-03", 10, DAY, |_| (None, Some(100.0), Some(100.0), Some(100.0), None));
        assert_eq!(line(&compute_study(&parse_studies("avwap", 12)[0], &flat), "weekly")[5], Some(100.0));

        let t0 = parse_iso("2026-08-03").unwrap();
        let heavy = vec![
            Candle { open_time: t0, open: 100.0, high: 100.0, low: 100.0, close: 100.0, volume: 1.0, quote_volume: None },
            Candle { open_time: t0 + DAY, open: 200.0, high: 200.0, low: 200.0, close: 200.0, volume: 99.0, quote_volume: None },
        ];
        assert!(line(&compute_study(&parse_studies("avwap", 12)[0], &heavy), "weekly")[1].unwrap() > 198.0);

        let daily = compute_study(&parse_studies("avwap", 12)[0], &series("2026-08-03", 10, DAY, |_| (None, None, None, None, None)));
        assert!(line(&daily, "session").iter().all(Option::is_none));
        let hourly = compute_study(&parse_studies("avwap", 12)[0], &series("2026-08-03", 10, HOUR, |_| (None, None, None, None, None)));
        assert!(line(&hourly, "session").iter().any(Option::is_some));

        let two_weeks = series("2026-08-03", 14, DAY, |i| {
            let p = if i < 7 { 100.0 } else { 200.0 };
            (None, Some(p), Some(p), Some(p), None)
        });
        let study = compute_study(&parse_studies("avwap", 12)[0], &two_weeks);
        assert_eq!(line(&study, "weekly")[7], Some(200.0));
        assert!(line(&study, "monthly")[7].unwrap() < 200.0);
    }

    #[test]
    fn swing_map_tags_confirmed_turns() {
        let mut prices = Vec::new();
        for leg in 0..4 {
            for _ in 0..10 {
                let last = prices.last().copied().unwrap_or(100.0);
                prices.push(if leg % 2 == 0 { last + 2.0 } else { last - 2.0 });
            }
        }
        let t0 = parse_iso("2026-08-03").unwrap();
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, &p)| Candle { open_time: t0 + i as i64 * DAY, open: p, high: p + 0.5, low: p - 0.5, close: p, volume: 1.0, quote_volume: None })
            .collect();
        let study = compute_study(&parse_studies("swingmap:3:3", 12)[0], &candles);
        assert!(study.markers.len() >= 2);
        assert!(study.markers.iter().all(|m| ["HH", "HL", "LH", "LL", "H", "L"].contains(&m.text.split(' ').next().unwrap())));
        assert!(study.markers.iter().any(|m| m.above) && study.markers.iter().any(|m| !m.above));

        let zig = line(&study, "zigzag");
        let first = zig.iter().position(Option::is_some).unwrap();
        let last = zig.iter().rposition(Option::is_some).unwrap();
        assert!(zig[first..=last].iter().all(Option::is_some));
        assert_eq!(study.stats.iter().map(|s| s.0).collect::<Vec<_>>(), ["Swings", "Avg rise", "Avg rise time", "Avg drop", "Avg drop time"]);
        let last_marker = study.markers.last().unwrap();
        let idx = candles.iter().position(|c| c.open_time == last_marker.time).unwrap();
        assert!(idx <= candles.len() - 1 - 3);
    }

    #[test]
    fn durations_read_to_the_hour() {
        assert_eq!(format_duration((2 * DAY + 4 * HOUR) as f64), "2d 4h");
        assert_eq!(format_duration((3 * DAY) as f64), "3d");
        assert_eq!(format_duration((6 * HOUR + 59 * 60_000) as f64), "6h");
    }
}
