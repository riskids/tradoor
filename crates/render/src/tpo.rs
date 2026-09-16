//! TPO profiles drawn in place of candles, with the options Exocharts gives them.
//!
//! The chart's own candles are the brackets and a split — a UTC day, a week, a
//! month — is how long a profile runs. Everything a profile is read by is worked
//! out once, when a bar changes: rows, volume at each row, value area by time or
//! by volume, initial balance, single prints, tails, and how far each level has
//! stayed untouched ("naked"). Painting only places it.

use tradoor_core::format::{format_price, format_turnover};
use tradoor_core::indicators::profile::value_area;
use tradoor_core::indicators::tpo::{tpo_by_session, tpo_letter, TpoOptions, TpoSession};
use tradoor_core::time::civil;
use tradoor_core::{Candle, ChartTimeframe};

use crate::color::Color;
use crate::paint::{Align, Baseline, Frame, Rect};
use crate::theme::Theme;
use crate::ticks::nice_step;

/// Rows the loaded range is cut into on auto row size.
const AUTO_ROWS: f64 = 500.0;
/// How much of its session the fullest profile's widest row spans, unexpanded.
const FILL: f64 = 0.92;
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    /// Value area in one colour, outside it the same or another.
    Mono,
    /// Rows shaded by how many brackets traded there.
    HeatMap,
    /// Each bracket green or red by its candle.
    CandleUpDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fading {
    Off,
    /// Early brackets faint, late ones strong.
    Time,
    /// Rows faint where little traded, strong where much did.
    Volume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceStats {
    Off,
    Tpo,
    Volume,
}

/// A single print or tail marker: shown, with text, from how many rows, in
/// which colour, and whether it runs on until price comes back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub show: bool,
    pub text: bool,
    pub min: usize,
    pub color: Color,
    pub naked: bool,
}

/// The options, under the names Exocharts uses. The page sends only what it
/// changed, as `key=value;key=value`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    // Main
    pub letters: bool,
    pub min_letter_height: f32,
    pub expand: bool,
    pub session_names: bool,
    pub open_marker: bool,
    pub close_marker: bool,
    /// Row size as ticks × this; 0 picks one from the loaded range.
    pub row_ticks: u32,
    // Colours
    pub color_mode: ColorMode,
    /// None: the theme's TPO colour.
    pub va_color: Option<Color>,
    pub outside_va: bool,
    pub outside_va_color: Color,
    pub heat_low: Color,
    pub heat_high: Color,
    pub fading: Fading,
    pub fading_min: f32,
    pub fading_max: f32,
    // POC, VA
    pub va_pct: f64,
    pub va_lines: bool,
    pub va_extend: bool,
    pub va_background: bool,
    pub va_background_color: Color,
    pub naked_va: bool,
    pub va_range_text: bool,
    /// None: the theme's POC colour.
    pub poc_color: Option<Color>,
    pub poc_ray: bool,
    pub naked_poc: bool,
    pub poc_price: bool,
    pub volume_va: bool,
    // Initial balance, stats
    pub ib: bool,
    pub ib_color: Color,
    pub ib_candles: usize,
    pub ib_lines: bool,
    pub ib_extend: bool,
    pub ib_background: bool,
    pub naked_ib: bool,
    pub ib_range_text: bool,
    pub price_stats: PriceStats,
    // Single prints, tails
    pub single_prints: Marker,
    pub sell_tail: Marker,
    pub buy_tail: Marker,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            letters: false,
            min_letter_height: 12.0,
            expand: false,
            session_names: false,
            open_marker: true,
            close_marker: false,
            row_ticks: 0,
            color_mode: ColorMode::Mono,
            va_color: None,
            outside_va: false,
            outside_va_color: Color::hex(0x4a7fd8),
            heat_low: Color::hex(0xd32f2f),
            heat_high: Color::hex(0xffd54f),
            fading: Fading::Time,
            fading_min: 0.3,
            fading_max: 1.0,
            va_pct: 70.0,
            va_lines: true,
            va_extend: true,
            va_background: false,
            va_background_color: Color::hex(0xd9dcd9),
            naked_va: false,
            va_range_text: true,
            poc_color: None,
            poc_ray: false,
            naked_poc: false,
            poc_price: false,
            volume_va: false,
            ib: false,
            ib_color: Color::hex(0x5ab6ff),
            ib_candles: 1,
            ib_lines: false,
            ib_extend: false,
            ib_background: false,
            naked_ib: false,
            ib_range_text: false,
            price_stats: PriceStats::Off,
            single_prints: Marker { show: false, text: false, min: 2, color: Color::hex(0xffd166), naked: false },
            sell_tail: Marker { show: false, text: false, min: 2, color: Color::hex(0xff4d5f), naked: false },
            buy_tail: Marker { show: false, text: false, min: 2, color: Color::hex(0x3ddc97), naked: false },
        }
    }
}

impl Settings {
    /// `key=value;key=value`, over the defaults. An unknown key or a value that
    /// does not parse is skipped, never an error: a setting from a newer page
    /// must not break an older engine.
    pub fn parse(raw: &str) -> Self {
        let mut s = Self::default();
        for pair in raw.split(';') {
            let Some((key, value)) = pair.split_once('=') else { continue };
            s.set(key.trim(), value.trim());
        }
        s
    }

    fn set(&mut self, key: &str, v: &str) {
        let flag = || match v {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        };
        let color = || Color::parse(v);
        let count = |lo: usize, hi: usize| v.parse::<usize>().ok().filter(|n| (lo..=hi).contains(n));
        macro_rules! apply {
            ($slot:expr, $value:expr) => {
                if let Some(x) = $value {
                    $slot = x;
                }
            };
        }
        match key {
            "letters" => apply!(self.letters, flag()),
            "minLetterHeight" => apply!(self.min_letter_height, v.parse::<f32>().ok().filter(|n| (1.0..=100.0).contains(n))),
            "expand" => apply!(self.expand, flag()),
            "sessionNames" => apply!(self.session_names, flag()),
            "openMarker" => apply!(self.open_marker, flag()),
            "closeMarker" => apply!(self.close_marker, flag()),
            "rowTicks" => apply!(self.row_ticks, v.parse::<u32>().ok().filter(|n| *n <= 1_000_000)),
            "colorMode" => apply!(
                self.color_mode,
                match v {
                    "mono" => Some(ColorMode::Mono),
                    "heatmap" => Some(ColorMode::HeatMap),
                    "candle" => Some(ColorMode::CandleUpDown),
                    _ => None,
                }
            ),
            "vaColor" => self.va_color = color(),
            "outsideVa" => apply!(self.outside_va, flag()),
            "outsideVaColor" => apply!(self.outside_va_color, color()),
            "heatLow" => apply!(self.heat_low, color()),
            "heatHigh" => apply!(self.heat_high, color()),
            "fading" => apply!(
                self.fading,
                match v {
                    "off" => Some(Fading::Off),
                    "time" => Some(Fading::Time),
                    "volume" => Some(Fading::Volume),
                    _ => None,
                }
            ),
            "fadingMin" => apply!(self.fading_min, v.parse::<f32>().ok().filter(|n| (0.0..=1.0).contains(n))),
            "fadingMax" => apply!(self.fading_max, v.parse::<f32>().ok().filter(|n| (0.0..=1.0).contains(n))),
            "vaPct" => apply!(self.va_pct, v.parse::<f64>().ok().filter(|n| (1.0..=100.0).contains(n))),
            "vaLines" => apply!(self.va_lines, flag()),
            "vaExtend" => apply!(self.va_extend, flag()),
            "vaBackground" => apply!(self.va_background, flag()),
            "vaBackgroundColor" => apply!(self.va_background_color, color()),
            "nakedVa" => apply!(self.naked_va, flag()),
            "vaRangeText" => apply!(self.va_range_text, flag()),
            "pocColor" => self.poc_color = color(),
            "pocRay" => apply!(self.poc_ray, flag()),
            "nakedPoc" => apply!(self.naked_poc, flag()),
            "pocPrice" => apply!(self.poc_price, flag()),
            "volumeVa" => apply!(self.volume_va, flag()),
            "ib" => apply!(self.ib, flag()),
            "ibColor" => apply!(self.ib_color, color()),
            "ibCandles" => apply!(self.ib_candles, count(1, 100)),
            "ibLines" => apply!(self.ib_lines, flag()),
            "ibExtend" => apply!(self.ib_extend, flag()),
            "ibBackground" => apply!(self.ib_background, flag()),
            "nakedIb" => apply!(self.naked_ib, flag()),
            "ibRangeText" => apply!(self.ib_range_text, flag()),
            "priceStats" => apply!(
                self.price_stats,
                match v {
                    "off" => Some(PriceStats::Off),
                    "tpo" => Some(PriceStats::Tpo),
                    "volume" => Some(PriceStats::Volume),
                    _ => None,
                }
            ),
            _ => {
                let (marker, field) = if let Some(f) = key.strip_prefix("singlePrint") {
                    (&mut self.single_prints, f)
                } else if let Some(f) = key.strip_prefix("sellTail") {
                    (&mut self.sell_tail, f)
                } else if let Some(f) = key.strip_prefix("buyTail") {
                    (&mut self.buy_tail, f)
                } else {
                    return;
                };
                match field {
                    "" | "s" => apply!(marker.show, flag()),
                    "Text" => apply!(marker.text, flag()),
                    "Min" => apply!(marker.min, count(1, 100)),
                    "Color" => apply!(marker.color, color()),
                    "Naked" => apply!(marker.naked, flag()),
                    _ => {}
                }
            }
        }
    }

    /// What the profiles themselves depend on; colours and lines do not rebuild them.
    fn shape(&self) -> (u32, u64, usize, bool, usize, usize, usize) {
        (self.row_ticks, self.va_pct.to_bits(), self.ib_candles, self.volume_va, self.single_prints.min, self.sell_tail.min, self.buy_tail.min)
    }
}

/// A band of rows, bottom and top row index inclusive.
pub type Band = (i64, i64);

/// One session's profile and everything read off it.
pub struct Profile {
    pub session: TpoSession,
    /// Index of its first and last bracket among the chart's bars.
    pub first: usize,
    pub last: usize,
    /// Volume at each of `session.rows`.
    pub volume: Vec<f64>,
    pub poc: f64,
    pub vah: f64,
    pub val: f64,
    pub ib: Option<(f64, f64)>,
    pub singles: Vec<Band>,
    pub sell_tail: Option<Band>,
    pub buy_tail: Option<Band>,
    pub open: f64,
    pub close: f64,
    /// The bar that first traded back to a level, or None while it is still naked.
    pub naked: Naked,
}

#[derive(Default, Debug, PartialEq)]
pub struct Naked {
    pub poc: Option<usize>,
    pub vah: Option<usize>,
    pub val: Option<usize>,
    pub ib_high: Option<usize>,
    pub ib_low: Option<usize>,
    pub singles: Vec<Option<usize>>,
    pub sell_tail: Option<usize>,
    pub buy_tail: Option<usize>,
}

pub struct Profiles {
    pub row_size: f64,
    pub grid_low: f64,
    /// Bar widths one bracket's block takes when not expanded.
    pub pitch: f64,
    pub list: Vec<Profile>,
    key: (ChartTimeframe, usize, i64, Option<(i64, u64, u64)>, (u32, u64, usize, bool, usize, usize, usize)),
}

impl Profiles {
    /// Whether these were built from exactly these bars and options.
    pub fn fresh(&self, session: ChartTimeframe, bars: &[Candle], settings: &Settings) -> bool {
        self.key == key(session, bars, settings)
    }
}

fn key(session: ChartTimeframe, bars: &[Candle], settings: &Settings) -> (ChartTimeframe, usize, i64, Option<(i64, u64, u64)>, (u32, u64, usize, bool, usize, usize, usize)) {
    let last = bars.last().map(|b| (b.open_time, b.high.to_bits(), b.low.to_bits()));
    (session, bars.len(), bars.first().map_or(0, |b| b.open_time), last, settings.shape())
}

/// Runs of rows where exactly one bracket traded, from `min` rows up.
fn single_runs(session: &TpoSession, min: usize) -> Vec<Band> {
    let mut out = Vec::new();
    let mut run: Option<Band> = None;
    for row in &session.rows {
        let single = row.periods.len() == 1;
        run = match (run, single) {
            (Some((lo, hi)), true) if row.index == hi + 1 => Some((lo, row.index)),
            (Some(done), _) => {
                out.push(done);
                single.then_some((row.index, row.index))
            }
            (None, true) => Some((row.index, row.index)),
            (None, false) => None,
        };
    }
    out.extend(run);
    out.retain(|(lo, hi)| (hi - lo + 1) as usize >= min.max(1));
    out
}

/// The first bar after `after` whose range reaches `low..=high`.
fn touched(bars: &[Candle], after: usize, low: f64, high: f64) -> Option<usize> {
    bars.iter().enumerate().skip(after + 1).find(|(_, b)| b.low <= high && b.high >= low).map(|(i, _)| i)
}

/// Every session's profile of `bars` — the chart's own candles, sorted — split
/// by `split`. `tick` is the smallest price step, for row sizes in ticks.
pub fn build(bars: &[Candle], split: ChartTimeframe, tick: f64, bar_ms: f64, settings: &Settings) -> Option<Profiles> {
    let high = bars.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
    let low = bars.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
    if !(high > low) {
        return None;
    }
    let row_size = if settings.row_ticks > 0 && tick > 0.0 { tick * f64::from(settings.row_ticks) } else { nice_step((high - low) / AUTO_ROWS) };
    // Too fine a row for the range would be millions of rows: fall back to auto.
    let row_size = if (high - low) / row_size > 20_000.0 { nice_step((high - low) / AUTO_ROWS) } else { row_size };
    let grid_low = (low / row_size).floor() * row_size;
    let opts = TpoOptions { min_periods: 1, value_area_pct: settings.va_pct / 100.0, initial_balance_periods: settings.ib_candles, ..TpoOptions::new(row_size, grid_low) };
    let row_of = |price: f64| ((price - grid_low) / row_size).floor() as i64;

    let mut list = Vec::new();
    let mut first = 0;
    for session in tpo_by_session(bars, split, &opts) {
        // A session's candles are consecutive among the bars, starting at its first period.
        while first < bars.len() && bars[first].open_time < session.periods[0] {
            first += 1;
        }
        let last = first + session.periods.len() - 1;
        let brackets = &bars[first..=last.min(bars.len() - 1)];

        let mut volume = vec![0.0; session.rows.len()];
        let base = session.rows.first().map_or(0, |r| r.index);
        let dense = |index: i64| session.rows.binary_search_by_key(&index, |r| r.index).ok();
        for b in brackets {
            let (lo, hi) = (row_of(b.low), row_of(b.high));
            let share = b.volume / (hi - lo + 1) as f64;
            for r in lo..=hi {
                if let Some(i) = dense(r) {
                    volume[i] += share;
                }
            }
        }

        let (poc, vah, val) = if settings.volume_va && volume.iter().any(|v| *v > 0.0) {
            // Contiguous, with holes as zeros, as the time value area is worked out.
            let top = session.rows.last().map_or(base, |r| r.index);
            let mut contiguous = vec![0.0; (top - base + 1) as usize];
            for (row, v) in session.rows.iter().zip(&volume) {
                contiguous[(row.index - base) as usize] = *v;
            }
            let poc_offset = contiguous.iter().enumerate().fold(0, |best, (i, v)| if *v > contiguous[best] { i } else { best });
            let (lo, hi) = value_area(&contiguous, poc_offset, settings.va_pct / 100.0);
            (grid_low + (base as f64 + poc_offset as f64 + 0.5) * row_size, grid_low + (base as f64 + hi as f64 + 1.0) * row_size, grid_low + (base as f64 + lo as f64) * row_size)
        } else {
            (session.poc, session.vah, session.val)
        };

        let mut singles = single_runs(&session, 1);
        let top_row = session.rows.last().map_or(0, |r| r.index);
        let bottom_row = session.rows.first().map_or(0, |r| r.index);
        let sell_tail = singles.iter().copied().find(|(_, hi)| *hi == top_row).filter(|(lo, hi)| (hi - lo + 1) as usize >= settings.sell_tail.min);
        let buy_tail = singles.iter().copied().find(|(lo, _)| *lo == bottom_row).filter(|(lo, hi)| (hi - lo + 1) as usize >= settings.buy_tail.min);
        // Tails are singles at an extreme; they are drawn as tails, not twice.
        singles.retain(|(lo, hi)| *hi != top_row && *lo != bottom_row && (hi - lo + 1) as usize >= settings.single_prints.min);

        let band = |(lo, hi): Band| (grid_low + lo as f64 * row_size, grid_low + (hi + 1) as f64 * row_size);
        let ib = session.initial_balance.map(|(h, l)| (h, l));
        let naked = Naked {
            poc: touched(bars, last, poc, poc),
            vah: touched(bars, last, vah, vah),
            val: touched(bars, last, val, val),
            ib_high: ib.and_then(|(h, _)| touched(bars, last, h, h)),
            ib_low: ib.and_then(|(_, l)| touched(bars, last, l, l)),
            singles: singles.iter().map(|s| band(*s)).map(|(lo, hi)| touched(bars, last, lo, hi)).collect(),
            sell_tail: sell_tail.map(band).and_then(|(lo, hi)| touched(bars, last, lo, hi)),
            buy_tail: buy_tail.map(band).and_then(|(lo, hi)| touched(bars, last, lo, hi)),
        };
        list.push(Profile {
            open: brackets.first().map_or(poc, |b| b.open),
            close: brackets.last().map_or(poc, |b| b.close),
            first,
            last,
            volume,
            poc,
            vah,
            val,
            ib,
            singles,
            sell_tail,
            buy_tail,
            naked,
            session,
        });
        first = last + 1;
    }

    // The widest blocks can be with the fullest profile's widest row still inside its session.
    let pitch = list
        .iter()
        .filter_map(|p| {
            let widest = p.session.rows.iter().map(|r| r.periods.len()).max()? as f64;
            Some((p.session.session_end - p.session.session_start) as f64 / bar_ms * FILL / widest)
        })
        .fold(f64::INFINITY, f64::min);
    let pitch = if pitch.is_finite() { pitch.max(1.0) } else { 1.0 };
    Some(Profiles { row_size, grid_low, pitch, list, key: key(split, bars, settings) })
}

/// Where profiles are drawn and with what.
pub struct View<'a> {
    pub plot: Rect,
    pub bar_spacing: f32,
    pub char_width: f32,
    pub theme: &'a Theme,
    pub precision: usize,
    /// Centre x of a bar by its index.
    pub x: &'a dyn Fn(f64) -> f32,
    /// y of a price.
    pub y: &'a dyn Fn(f64) -> f32,
    pub bars: &'a [Candle],
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color { r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b), a: ch(a.a, b.a) }
}

/// Draws every profile with any part on screen.
pub fn paint(f: &mut Frame, v: &View, p: &Profiles, s: &Settings) {
    let t = v.theme;
    let plot_w = v.plot.w;
    let dpr = f.dpr;
    let hair = 1.0 / dpr;
    let bar = v.bar_spacing;
    let slot = if s.expand { bar } else { bar * p.pitch as f32 };
    let block = f.snap((slot * 0.62).max(hair)).max(hair);
    let merged = slot * dpr < 1.5;
    let va_color = s.va_color.unwrap_or(t.tpo);
    let poc_color = s.poc_color.unwrap_or(t.tpo_poc);
    let label_size = t.font_px - 2.0;
    let glyph = v.char_width * label_size / t.font_px;
    let decimals = (0..=8usize).find(|d| {
        let scaled = p.row_size * 10f64.powi(*d as i32);
        (scaled - scaled.round()).abs() < 1e-6
    });
    let price = |x: f64| format_price(x, decimals);
    let right_edge = plot_w;
    // x where a level stops: the bar that traded back into it, or the chart's edge.
    let naked_end = |touch: Option<usize>| touch.map_or(right_edge, |i| (v.x)(i as f64));
    let starts: Vec<f32> = p.list.iter().map(|profile| (v.x)(profile.first as f64) - bar / 2.0).collect();

    for (k, profile) in p.list.iter().enumerate() {
        let session = &profile.session;
        let x0 = starts[k];
        let widest = session.rows.iter().map(|r| r.periods.len()).max().unwrap_or(0) as f32 * slot;
        let label_w = format!("VAH {}", price(profile.vah)).len() as f32 * glyph;
        let end = starts.get(k + 1).copied().unwrap_or(x0 + widest + label_w + 16.0);
        let profile_right = (x0 + widest).min(end);
        let reach_naked = s.naked_poc || s.naked_va || s.naked_ib || s.single_prints.naked || s.sell_tail.naked || s.buy_tail.naked;
        if x0 > plot_w || (end < 0.0 && !reach_naked) {
            continue;
        }
        let y_top = |index: i64| (v.y)(p.grid_low + (index + 1) as f64 * p.row_size);
        let y_bottom = |index: i64| (v.y)(p.grid_low + index as f64 * p.row_size);
        let last_period = session.periods.len().saturating_sub(1).max(1) as f32;
        let max_count = session.rows.iter().map(|r| r.periods.len()).max().unwrap_or(1).max(1) as f32;
        let max_volume = profile.volume.iter().copied().fold(0.0, f64::max);
        let poc_row = ((profile.poc - p.grid_low) / p.row_size).floor() as i64;
        let (vah, val) = (profile.vah, profile.val);

        // Value area background, under the blocks.
        if s.va_background {
            let right = if s.naked_va { naked_end(profile.naked.vah.max(profile.naked.val)) } else if s.va_extend { end } else { profile_right };
            f.rect(x0, (v.y)(vah), (right - x0).max(0.0), (v.y)(val) - (v.y)(vah), s.va_background_color.with_alpha(0x1c));
        }
        if s.ib && s.ib_background {
            if let Some((h, l)) = profile.ib {
                let right = if s.naked_ib { naked_end(profile.naked.ib_high.max(profile.naked.ib_low)) } else if s.ib_extend { end } else { profile_right };
                f.rect(x0, (v.y)(h), (right - x0).max(0.0), (v.y)(l) - (v.y)(h), s.ib_color.with_alpha(0x1c));
            }
        }
        for (marker, band, touch) in [(s.sell_tail, profile.sell_tail, profile.naked.sell_tail), (s.buy_tail, profile.buy_tail, profile.naked.buy_tail)] {
            if let (true, Some((lo, hi))) = (marker.show, band) {
                let right = if marker.naked { naked_end(touch) } else { profile_right };
                f.rect(x0, y_top(hi), (right - x0).max(0.0), y_bottom(lo) - y_top(hi), marker.color.with_alpha(0x30));
                if marker.text {
                    let name = if hi == session.rows.last().map_or(0, |r| r.index) { "Sell tail" } else { "Buy tail" };
                    f.text(right - 2.0, y_top(hi) - 1.0, name, marker.color, Align::Right, Baseline::Bottom, label_size, None, Some(v.plot));
                }
            }
        }
        if s.single_prints.show {
            for (i, (lo, hi)) in profile.singles.iter().enumerate() {
                let right = if s.single_prints.naked { naked_end(profile.naked.singles.get(i).copied().flatten()) } else { profile_right };
                f.rect(x0, y_top(*hi), (right - x0).max(0.0), y_bottom(*lo) - y_top(*hi), s.single_prints.color.with_alpha(0x30));
                if s.single_prints.text {
                    f.text(right - 2.0, y_top(*hi) - 1.0, format!("SP {}", price(p.grid_low + *lo as f64 * p.row_size)), s.single_prints.color, Align::Right, Baseline::Bottom, label_size, None, Some(v.plot));
                }
            }
        }

        // The session's range, a hairline down its left edge.
        f.vline(f.snap(x0), (v.y)(session.high), (v.y)(session.low), hair, va_color.with_alpha(0x55));

        for (ri, row) in session.rows.iter().enumerate() {
            let top = y_top(row.index);
            let bottom = y_bottom(row.index);
            if bottom < v.plot.y || top > v.plot.bottom() {
                continue;
            }
            let height = bottom - top;
            let gap = if height >= 3.0 { hair } else { 0.0 };
            let h = (height - gap).max(hair);
            let mid = p.grid_low + (row.index as f64 + 0.5) * p.row_size;
            let in_va = mid >= val && mid <= vah;
            let poc = row.index == poc_row;
            let count = row.periods.len() as f32;
            let color_of = |period: usize| -> Color {
                let base = if poc {
                    poc_color
                } else {
                    match s.color_mode {
                        ColorMode::Mono => {
                            if !in_va && s.outside_va {
                                s.outside_va_color
                            } else {
                                va_color
                            }
                        }
                        ColorMode::HeatMap => mix(s.heat_low, s.heat_high, count / max_count),
                        ColorMode::CandleUpDown => match v.bars.get(profile.first + period) {
                            Some(b) if b.close < b.open => t.bear,
                            _ => t.bull,
                        },
                    }
                };
                let strength = match s.fading {
                    Fading::Off => 0.85,
                    Fading::Time => period as f32 / last_period,
                    Fading::Volume => (if max_volume > 0.0 { profile.volume[ri] / max_volume } else { 1.0 }) as f32,
                };
                let strength = if matches!(s.fading, Fading::Off) { strength } else { s.fading_min + (s.fading_max - s.fading_min) * strength.clamp(0.0, 1.0) };
                base.with_alpha((255.0 * strength.clamp(0.05, 1.0)) as u8)
            };

            // Letters where a row is tall and a bracket wide enough to read one.
            let letter_px = (height - 2.0).min(t.font_px + 1.0);
            if s.letters && height >= s.min_letter_height && slot >= v.char_width * letter_px / t.font_px * 0.9 {
                for (j, period) in row.periods.iter().enumerate() {
                    let x = x0 + (j as f32 + 0.5) * slot;
                    if x > plot_w {
                        break;
                    }
                    f.text(x, (top + bottom) / 2.0, tpo_letter(*period).to_string(), color_of(*period).with_alpha(255), Align::Center, Baseline::Middle, letter_px, None, Some(v.plot));
                }
            } else if merged {
                // Brackets narrower than a device pixel: a few pixels at a time,
                // each shaded by the brackets it stands for.
                let per = ((3.0 / dpr / slot).ceil() as usize).max(1);
                for (c, chunk) in row.periods.chunks(per).enumerate() {
                    let x = x0 + (c * per) as f32 * slot;
                    if x > plot_w {
                        break;
                    }
                    let mean = (chunk.iter().sum::<usize>() as f32 / chunk.len() as f32).round() as usize;
                    f.rect(x, top, (chunk.len() as f32 * slot).max(hair), h, color_of(mean));
                }
            } else {
                for (j, period) in row.periods.iter().enumerate() {
                    let x = f.snap(x0 + j as f32 * slot + (slot - block) / 2.0);
                    if x > plot_w {
                        break;
                    }
                    f.rect(x, top, block, h, color_of(*period));
                }
            }

            // Price statistics beside the row, where there is height to read them.
            if s.price_stats != PriceStats::Off && height >= 9.0 {
                let text = match s.price_stats {
                    PriceStats::Tpo => row.periods.len().to_string(),
                    _ => format_turnover(profile.volume[ri]),
                };
                f.text(x0 + count * slot + 3.0, (top + bottom) / 2.0, text, t.text, Align::Left, Baseline::Middle, label_size.min(height - 1.0), None, Some(v.plot));
            }
        }

        // POC ray and price.
        if s.poc_ray || s.naked_poc {
            let right = if s.naked_poc { naked_end(profile.naked.poc) } else { end };
            f.hline(x0, right.min(plot_w), (v.y)(profile.poc), 1.0, poc_color);
        }
        if s.poc_price {
            let right = if s.naked_poc { naked_end(profile.naked.poc) } else { end }.min(plot_w);
            f.text(right - 2.0, (v.y)(profile.poc) - 2.0, format!("POC {}", price(profile.poc)), poc_color, Align::Right, Baseline::Bottom, label_size, None, Some(v.plot));
        }

        // Value area lines, labelled at the end.
        let room = end - x0;
        for (name, level, touch) in [("VAH", vah, profile.naked.vah), ("VAL", val, profile.naked.val)] {
            let y = (v.y)(level);
            if !(v.plot.y..=v.plot.bottom()).contains(&y) || room < 8.0 {
                continue;
            }
            let right = if s.naked_va { naked_end(touch) - 2.0 } else if s.va_extend { end - 6.0 } else { profile_right }.min(plot_w);
            if s.va_lines || s.naked_va {
                f.dashed_line(x0.max(0.0), y, right, y, hair, va_color.with_alpha(0x70), 4.0, 3.0);
            }
            if s.va_range_text && (right - x0) >= label_w + 12.0 {
                f.text(right - 2.0, y - 2.0, format!("{name} {}", price(level)), va_color.with_alpha(0xd0), Align::Right, Baseline::Bottom, label_size, None, Some(v.plot));
            }
        }

        // Initial balance: the first brackets' range as a bar by the profile, and its lines.
        if s.ib {
            if let Some((h, l)) = profile.ib {
                f.rect(f.snap(x0 - 7.0), (v.y)(h), 3.0, ((v.y)(l) - (v.y)(h)).max(2.0), s.ib_color);
                for (level, touch) in [(h, profile.naked.ib_high), (l, profile.naked.ib_low)] {
                    if s.ib_lines || s.naked_ib {
                        let right = if s.naked_ib { naked_end(touch) } else if s.ib_extend { end } else { profile_right };
                        f.dashed_line(x0.max(0.0), (v.y)(level), right.min(plot_w), (v.y)(level), hair, s.ib_color.with_alpha(0xa0), 2.0, 2.0);
                    }
                }
                if s.ib_range_text {
                    f.text(x0 - 9.0, (v.y)(h), format!("IB {}–{}", price(l), price(h)), s.ib_color, Align::Right, Baseline::Top, label_size, None, Some(v.plot));
                }
            }
        }

        // Where the session opened and closed: arrows at its left edge.
        if s.open_marker {
            let y = (v.y)(profile.open);
            f.triangle((x0 - 2.0, y), (x0 - 7.0, y - 4.0), (x0 - 7.0, y + 4.0), t.bear);
        }
        if s.close_marker {
            let y = (v.y)(profile.close);
            let x = profile_right + 3.0;
            f.triangle((x, y), (x + 5.0, y - 4.0), (x + 5.0, y + 4.0), t.bull);
        }
        if s.session_names {
            let c = civil(session.session_start);
            f.text(x0, (v.y)(session.high) - 3.0, format!("{} {}", MONTHS[c.month0 as usize], c.day), t.text, Align::Left, Baseline::Bottom, label_size, None, Some(v.plot));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tradoor_core::time::HOUR;

    fn bar(i: i64, low: f64, high: f64, volume: f64) -> Candle {
        Candle { open_time: i * HOUR, open: low, high, low, close: high, volume, quote_volume: None }
    }

    #[test]
    fn settings_are_read_over_the_defaults_and_nonsense_is_skipped() {
        let s = Settings::parse("letters=1;colorMode=heatmap;vaPct=68;ibCandles=2;sellTail=1;sellTailMin=3;buyTailColor=#112233;bogus=9;vaPct=999");
        assert!(s.letters);
        assert_eq!(s.color_mode, ColorMode::HeatMap);
        assert_eq!(s.va_pct, 68.0, "an out-of-range value leaves the last good one");
        assert_eq!(s.ib_candles, 2);
        assert!(s.sell_tail.show && s.sell_tail.min == 3);
        assert_eq!(s.buy_tail.color, Color::hex(0x112233));
        assert_eq!(Settings::parse(""), Settings::default());
    }

    /// A day of hourly brackets: a thin run up, a fat middle, a thin run down.
    fn day() -> Vec<Candle> {
        let mut bars = vec![bar(0, 100.0, 100.5, 10.0)];
        for i in 1..20 {
            bars.push(bar(i, 104.0, 106.0, 50.0));
        }
        bars.push(bar(20, 108.0, 112.0, 10.0));
        bars
    }

    #[test]
    fn profiles_find_the_value_area_tails_and_single_prints() {
        let bars = day();
        let s = Settings { row_ticks: 10, sell_tail: Marker { min: 2, ..Settings::default().sell_tail }, ..Settings::default() };
        let p = build(&bars, ChartTimeframe::D1, 0.1, HOUR as f64, &s).expect("a profile");
        assert_eq!(p.row_size, 1.0, "ten ticks of 0.1");
        let profile = &p.list[0];
        assert!(profile.val >= 104.0 && profile.vah <= 107.0, "value area around the fat middle: {}–{}", profile.val, profile.vah);
        let prices = |(lo, hi): Band| (p.grid_low + lo as f64 * p.row_size, p.grid_low + hi as f64 * p.row_size);
        assert_eq!(profile.sell_tail.map(prices), Some((108.0, 112.0)), "the last bracket alone at the top is a selling tail");
        assert!(profile.buy_tail.is_none(), "one row at the bottom is not a tail of two");
        assert!(profile.singles.is_empty(), "the lone first bracket touches the bottom, so it is no single print inside");
        assert_eq!(profile.ib, Some((100.5, 100.0)), "the first bracket's range");
    }

    #[test]
    fn a_level_stays_naked_until_a_later_bar_trades_back_to_it() {
        let mut bars = day();
        // The next day stays above the value area, then comes back down to it.
        for i in 24..30 {
            bars.push(bar(i, 110.0, 115.0, 5.0));
        }
        bars.push(bar(30, 104.5, 111.0, 5.0));
        let p = build(&bars, ChartTimeframe::D1, 0.1, HOUR as f64, &Settings { row_ticks: 10, ..Settings::default() }).unwrap();
        assert_eq!(p.list[0].naked.poc, Some(bars.len() - 1), "the POC is naked until the last bar reaches it");
        assert_eq!(p.list[1].naked.poc, None, "today's POC has nothing after it yet");
    }

    #[test]
    fn volume_can_decide_the_value_area_instead_of_time() {
        let mut bars = vec![];
        // Many quiet brackets at 100, a few heavy ones at 120.
        for i in 0..10 {
            bars.push(bar(i, 100.0, 100.5, 1.0));
        }
        for i in 10..13 {
            bars.push(bar(i, 120.0, 120.5, 1_000.0));
        }
        let by_time = build(&bars, ChartTimeframe::D1, 0.1, HOUR as f64, &Settings { row_ticks: 10, ..Settings::default() }).unwrap();
        let by_volume = build(&bars, ChartTimeframe::D1, 0.1, HOUR as f64, &Settings { row_ticks: 10, volume_va: true, ..Settings::default() }).unwrap();
        assert!(by_time.list[0].poc < 110.0, "most time at 100");
        assert!(by_volume.list[0].poc > 110.0, "most volume at 120");
    }
}
