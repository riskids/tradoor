//! The chart: data, view, interaction, and the frame that draws them.

use std::collections::{BTreeMap, BTreeSet};

use tradoor_core::chart::{engine_series, EngineSeries};
use tradoor_core::format::{format_countdown, format_price, format_turnover, price_precision};
use tradoor_core::live::LiveBar;
use tradoor_core::studies::{compute_study, parse_studies, ComputedStudy, Group, LineStyle, LineType, StudySpec, Tone};
use tradoor_core::time::DAY;
use tradoor_core::{Candle, ChartTimeframe, Interval};

use crate::color::Color;
use crate::drawings::{self, Anchor, DrawingKind, DrawingShape, Placed, FIB_LEVELS, MAX_POINTS};
use crate::geometry::{distance_to_drawing, logical_to_time, ribbon_runs, time_to_logical, visible_slice, RibbonRun};
use crate::paint::{Align, Baseline, Frame, Rect};
use crate::scale::{PriceScale, ScaleMode, TimeScale};
use crate::theme::Theme;
use crate::ticks;

const TIME_AXIS_HEIGHT: f32 = 22.0;
const MIN_AXIS_WIDTH: f32 = 56.0;
const MIN_PANE_HEIGHT: f32 = 40.0;
const SEPARATOR_GRAB: f32 = 4.0;
/// A chart opens on this many bars: all of them at once makes every candle a
/// hairline, which shows the trend and nothing about any setup.
const DEFAULT_VISIBLE_BARS: f64 = 150.0;
const RIGHT_MARGIN_BARS: f64 = 2.0;
const CLICK_SLOP: f32 = 5.0;
/// How far a drag must move vertically before it takes an autoscaled pane's
/// price axis over, so a sideways pan with a shaky hand keeps autoscale.
const VERTICAL_PAN_SLOP: f32 = 24.0;
const HIT_TOLERANCE: f32 = 6.0;
const BRUSH_SAMPLE: f32 = 3.0;
/// Ask for older history once fewer than this many bars remain off the left edge.
const HISTORY_MARGIN_BARS: f64 = 80.0;
const MARGIN_STEP: f64 = 0.04;
const MAX_MARGIN: f64 = 0.45;

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The last anchor landed; the page saves it. A `text` drawing still needs its label.
    DrawingCreated { kind: DrawingKind, points: Vec<Anchor> },
    SelectionChanged(Option<i64>),
    DeleteRequested(i64),
    /// A tool finished or was cancelled; the palette falls back to select.
    ToolFinished,
    /// The view is near the oldest bar held.
    NeedHistory { before: i64 },
    PaneWeightsChanged(String),
    /// Every pane follows the bars on screen again, or one stopped doing so.
    AutoScaleChanged(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Crosshair,
    Pointer,
    Grabbing,
    ColResize,
    RowResize,
    NsResize,
}

impl Cursor {
    pub fn css(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Crosshair => "crosshair",
            Self::Pointer => "pointer",
            Self::Grabbing => "grabbing",
            Self::ColResize => "ew-resize",
            Self::RowResize => "row-resize",
            Self::NsResize => "ns-resize",
        }
    }
}

/// The OHLC readout for the bar under the pointer, or the newest bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Legend {
    pub index: Option<usize>,
    pub time: i64,
    pub ohlc: String,
    pub change: String,
    pub up: bool,
    pub volume: String,
    /// `(studyId:lineKey, formatted value)`.
    pub values: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudyMeta {
    pub id: String,
    pub label: String,
    pub group: &'static str,
    pub hidden: bool,
    /// `(studyId:lineKey, label, colour)`.
    pub lines: Vec<(String, String, Color)>,
    pub stats: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PaneState {
    weight: f64,
    auto: bool,
    margins: (f64, f64),
    manual: Option<(f64, f64)>,
}

impl PaneState {
    fn default_for(group: Group) -> Self {
        match group {
            Group::Price => Self { weight: 3.0, auto: true, margins: (0.2, 0.1), manual: None },
            _ => Self { weight: 1.0, auto: true, margins: (0.08, 0.08), manual: None },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PaneView {
    group: Group,
    plot: Rect,
    scale: PriceScale,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    None,
    /// Pressed on a plot. It is a click until it moves past the slop, then a pan;
    /// `price_pan` once the pane's price follows the pointer too.
    Press { start: (f32, f32), last_x: f32, last_y: f32, pane: usize, panning: bool, price_pan: bool },
    PriceAxis { pane: usize, last_y: f32 },
    TimeAxis { last_x: f32 },
    Separator { upper: usize, last_y: f32 },
    Brush { last: (f32, f32) },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    Plot(usize),
    PriceAxis(usize),
    TimeAxis,
    Separator(usize),
    Outside,
}

#[derive(Clone, Copy, Debug)]
struct Bar {
    time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
    live: bool,
}

pub struct Chart {
    theme: Theme,
    char_width: f32,
    width: f32,
    height: f32,
    dpr: f32,
    timeframe: ChartTimeframe,
    candles: Vec<Candle>,
    times: Vec<f64>,
    live: Option<LiveBar>,
    precision: usize,
    precision_override: Option<usize>,
    engine: EngineSeries,
    ribbon: Vec<RibbonRun>,
    study_specs: Vec<StudySpec>,
    studies: Vec<ComputedStudy>,
    hidden: BTreeSet<String>,
    log_scale: bool,
    time: TimeScale,
    view_ready: bool,
    pane_state: BTreeMap<Group, PaneState>,
    panes: Vec<PaneView>,
    axis_width: f32,
    crosshair: Option<(f32, f32)>,
    drag: Drag,
    drawings: Vec<DrawingShape>,
    selected: Option<i64>,
    tool: Option<DrawingKind>,
    placed: Vec<Anchor>,
    draft: Option<DrawingShape>,
    events: Vec<Event>,
    history_asked: Option<i64>,
    message: Option<String>,
    dirty: bool,
    last_second: i64,
    cursor: Cursor,
    frame: Frame,
}

impl Chart {
    pub fn new(timeframe: ChartTimeframe) -> Self {
        Self {
            theme: Theme::default(),
            char_width: 6.6,
            width: 0.0,
            height: 0.0,
            dpr: 1.0,
            timeframe,
            candles: Vec::new(),
            times: Vec::new(),
            live: None,
            precision: 2,
            precision_override: None,
            engine: engine_series(&[]),
            ribbon: Vec::new(),
            study_specs: Vec::new(),
            studies: Vec::new(),
            hidden: BTreeSet::new(),
            log_scale: false,
            time: TimeScale::new(),
            view_ready: false,
            pane_state: BTreeMap::new(),
            panes: Vec::new(),
            axis_width: MIN_AXIS_WIDTH,
            crosshair: None,
            drag: Drag::None,
            drawings: Vec::new(),
            selected: None,
            tool: None,
            placed: Vec::new(),
            draft: None,
            events: Vec::new(),
            history_asked: None,
            message: None,
            dirty: true,
            last_second: 0,
            cursor: Cursor::Crosshair,
            frame: Frame::default(),
        }
    }

    fn touch(&mut self) {
        self.dirty = true;
    }

    // ------------------------------------------------------------------ inputs

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn set_theme_value(&mut self, key: &str, value: &str) -> bool {
        let ok = self.theme.set(key, value);
        self.dirty |= ok;
        ok
    }

    /// The width of one glyph of the axis font, measured by the backend. Axis
    /// labels are monospace, so this sizes every label box exactly.
    pub fn set_char_width(&mut self, width: f32) {
        if width > 1.0 {
            self.char_width = width;
            self.touch();
        }
    }

    pub fn resize(&mut self, width: f32, height: f32, dpr: f32) {
        self.width = width.max(0.0);
        self.height = height.max(0.0);
        self.dpr = dpr.max(0.5);
        self.touch();
    }

    /// Shown in place of the chart while there are no bars.
    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
        self.touch();
    }

    pub fn timeframe(&self) -> ChartTimeframe {
        self.timeframe
    }

    pub fn set_timeframe(&mut self, timeframe: ChartTimeframe) {
        if timeframe != self.timeframe {
            self.timeframe = timeframe;
            self.view_ready = false;
            self.live = None;
            self.placed.clear();
            self.draft = None;
            self.touch();
        }
    }

    /// Replace the closed bars. Older bars prepended keep the view where it is:
    /// the right offset is measured from the newest bar, which has not moved.
    pub fn set_candles(&mut self, candles: Vec<Candle>) {
        let old_first = self.candles.first().map(|c| c.open_time);
        self.candles = candles;
        self.times = self.candles.iter().map(|c| c.open_time as f64).collect();
        if let (Some(live), Some(last)) = (self.live, self.candles.last()) {
            if live.open_time <= last.open_time {
                self.live = None;
            }
        }
        if self.candles.first().map(|c| c.open_time) != old_first {
            self.history_asked = None;
        }
        self.precision = price_precision(self.candles.iter().rev().take(2000).flat_map(|c| [c.open, c.high, c.low, c.close]));
        self.engine = engine_series(&self.candles);
        self.ribbon = ribbon_runs(&self.engine.ema12, &self.engine.ema25);
        self.studies = self.study_specs.iter().map(|s| compute_study(s, &self.candles)).collect();
        if self.candles.is_empty() {
            self.view_ready = false;
        }
        self.touch();
    }

    pub fn candles(&self) -> &[Candle] {
        &self.candles
    }

    /// The forming bar, drawn after the closed ones. Indicators stop at the last
    /// CLOSED bar: the forming one can still change.
    pub fn set_live(&mut self, live: Option<LiveBar>) {
        let live = live.filter(|l| self.candles.last().is_none_or(|c| l.open_time > c.open_time));
        if live != self.live {
            self.live = live;
            self.touch();
        }
    }

    /// `ema:50,rsi:14`.
    pub fn set_studies(&mut self, raw: &str) {
        let specs = parse_studies(raw, 12);
        if specs != self.study_specs {
            self.studies = specs.iter().map(|s| compute_study(s, &self.candles)).collect();
            self.study_specs = specs;
            self.touch();
        }
    }

    /// Hidden layer ids: `ribbon`, `ema200`, `volume`, or a study id.
    pub fn set_hidden(&mut self, raw: &str) {
        self.hidden = raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        self.touch();
    }

    pub fn set_log_scale(&mut self, log: bool) {
        if log != self.log_scale {
            self.log_scale = log;
            // A manual range is kept in the old mode's space and means nothing in the new one.
            let was = self.auto_scale();
            self.pane_state.insert(Group::Price, PaneState { weight: self.weight(Group::Price), ..PaneState::default_for(Group::Price) });
            self.note_auto(was);
            self.touch();
        }
    }

    pub fn log_scale(&self) -> bool {
        self.log_scale
    }

    /// The exchange's tick decimals, when known; otherwise read off the prices.
    pub fn set_price_decimals(&mut self, decimals: Option<usize>) {
        self.precision_override = decimals.map(|d| d.clamp(0, 10));
        self.touch();
    }

    pub fn precision(&self) -> usize {
        self.precision_override.unwrap_or(self.precision)
    }

    fn weight(&self, group: Group) -> f64 {
        self.pane_state.get(&group).map_or(PaneState::default_for(group).weight, |s| s.weight)
    }

    /// `price:3,pct:1` — pane heights as relative weights.
    pub fn set_pane_weights(&mut self, raw: &str) {
        for (key, weight) in raw.split(',').filter_map(|p| p.split_once(':')) {
            let group = match key.trim() {
                "price" => Group::Price,
                "pct" => Group::Pct,
                "macd" => Group::Macd,
                "atr" => Group::Atr,
                _ => continue,
            };
            if let Ok(w) = weight.trim().parse::<f64>() {
                self.pane_state.entry(group).or_insert_with(|| PaneState::default_for(group)).weight = w.clamp(0.1, 50.0);
            }
        }
        self.touch();
    }

    pub fn pane_weights(&self) -> String {
        self.panes.iter().map(|p| format!("{}:{:.3}", p.group.as_str(), self.weight(p.group))).collect::<Vec<_>>().join(",")
    }

    pub fn set_drawings(&mut self, shapes: Vec<DrawingShape>) {
        if self.selected.is_some_and(|id| !shapes.iter().any(|s| s.id == Some(id))) {
            self.selected = None;
        }
        self.drawings = shapes;
        self.touch();
    }

    pub fn set_selected(&mut self, id: Option<i64>) {
        self.selected = id;
        self.touch();
    }

    pub fn selected(&self) -> Option<i64> {
        self.selected
    }

    pub fn set_tool(&mut self, tool: Option<DrawingKind>) {
        self.tool = tool;
        self.placed.clear();
        self.draft = None;
        self.drag = Drag::None;
        self.touch();
    }

    pub fn tool(&self) -> Option<DrawingKind> {
        self.tool
    }

    /// Back to the opening view: the last bars, and every axis autoscaling.
    pub fn reset_view(&mut self) {
        self.view_ready = false;
        let was = self.auto_scale();
        for (group, state) in self.pane_state.iter_mut() {
            *state = PaneState { weight: state.weight, ..PaneState::default_for(*group) };
        }
        self.note_auto(was);
        self.touch();
    }

    /// Zoom to the last `days` of bars (all of them for `None`), counted from the
    /// candles so a gap in the tape cannot stretch the view past the data.
    pub fn show_range(&mut self, days: Option<f64>) {
        let n = self.candles.len();
        if n == 0 {
            return;
        }
        let bars = match days {
            None => n,
            Some(d) => {
                let from = self.candles[n - 1].open_time as f64 - d * DAY as f64;
                self.candles.iter().rev().take_while(|c| c.open_time as f64 >= from).count().max(8.min(n))
            }
        };
        self.time.show_last(bars as f64 + if self.live.is_some() { 1.0 } else { 0.0 }, RIGHT_MARGIN_BARS);
        self.view_ready = true;
        let was = self.auto_scale();
        if let Some(state) = self.pane_state.get_mut(&Group::Price) {
            state.auto = true;
            state.manual = None;
        }
        self.note_auto(was);
        self.touch();
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Something changed, or the countdown needs its next second.
    pub fn needs_render(&self, now: i64) -> bool {
        self.dirty || (!self.candles.is_empty() && now.div_euclid(1000) != self.last_second)
    }

    // ------------------------------------------------------------------ layout

    fn visible_studies(&self) -> impl Iterator<Item = (usize, &ComputedStudy)> {
        self.studies.iter().filter(|s| !self.hidden.contains(&s.id)).enumerate()
    }

    fn total_bars(&self) -> usize {
        self.candles.len() + usize::from(self.live.is_some())
    }

    fn bar(&self, i: usize) -> Option<Bar> {
        if let Some(c) = self.candles.get(i) {
            return Some(Bar { time: c.open_time, open: c.open, high: c.high, low: c.low, close: c.close, volume: c.volume, live: false });
        }
        self.live.filter(|_| i == self.candles.len()).map(|l| Bar { time: l.open_time, open: l.open, high: l.high, low: l.low, close: l.close, volume: l.volume, live: true })
    }

    fn plot_width(&self) -> f32 {
        self.panes.first().map_or((self.width - self.axis_width).max(10.0), |p| p.plot.w)
    }

    fn time_axis_top(&self) -> f32 {
        (self.height - TIME_AXIS_HEIGHT).max(20.0)
    }

    fn layout(&mut self) {
        let mut groups = vec![Group::Price];
        for (_, s) in self.visible_studies() {
            if !groups.contains(&s.group) {
                groups.push(s.group);
            }
        }
        for g in &groups {
            self.pane_state.entry(*g).or_insert_with(|| PaneState::default_for(*g));
        }

        let p = self.precision();
        let (hi, lo) = self.candles.iter().fold((f64::NEG_INFINITY, f64::INFINITY), |(h, l), c| (h.max(c.high), l.min(c.low)));
        let widest = [format_price(hi, Some(p)), format_price(lo, Some(p)), "100.00".to_string()].iter().map(String::len).max().unwrap_or(6);
        self.axis_width = (widest as f32 * self.char_width + 18.0).max(MIN_AXIS_WIDTH).min(self.width * 0.45);
        let plot_w = (self.width - self.axis_width).max(10.0);
        let avail = self.time_axis_top();

        let total: f64 = groups.iter().map(|g| self.weight(*g)).sum();
        let mut panes = Vec::with_capacity(groups.len());
        let mut y = 0.0f32;
        for (i, g) in groups.iter().enumerate() {
            let h = if i + 1 == groups.len() { avail - y } else { ((f64::from(avail) * self.weight(*g) / total) as f32).max(MIN_PANE_HEIGHT) };
            let state = self.pane_state[g];
            let mode = if *g == Group::Price && self.log_scale { ScaleMode::Log } else { ScaleMode::Normal };
            panes.push(PaneView { group: *g, plot: Rect { x: 0.0, y, w: plot_w, h: h.max(1.0) }, scale: PriceScale::new(mode, f64::from(y), f64::from(h.max(1.0)), state.margins) });
            y += h;
        }

        self.time.width = f64::from(plot_w);
        self.time.bars = self.total_bars();
        if !self.view_ready && self.time.bars > 0 {
            self.time.show_last(DEFAULT_VISIBLE_BARS, RIGHT_MARGIN_BARS);
            self.view_ready = true;
            // New bars or a reset view: a price range set by hand for the old
            // view would leave the new candles off screen.
            let was = self.auto_scale();
            for state in self.pane_state.values_mut() {
                state.auto = true;
                state.manual = None;
            }
            self.note_auto(was);
        } else {
            self.time.clamp();
        }

        let (i0, i1) = self.visible_indices(0.0);
        for pane in &mut panes {
            let state = self.pane_state[&pane.group];
            match (state.auto, state.manual) {
                (false, Some(range)) => pane.scale.range = range,
                _ => {
                    let (lo, hi) = self.pane_extent(pane.group, i0, i1);
                    pane.scale.fit(lo, hi);
                }
            }
        }
        self.panes = panes;
    }

    /// Bar indices overlapping the plot, widened by `extra` bars each side.
    fn visible_indices(&self, extra: f64) -> (usize, usize) {
        let n = self.total_bars();
        if n == 0 {
            return (0, 0);
        }
        let (from, to) = self.time.visible();
        let i0 = (from.floor() - extra).clamp(0.0, (n - 1) as f64) as usize;
        let i1 = (to.ceil() + extra).clamp(0.0, (n - 1) as f64) as usize;
        (i0, i1)
    }

    fn pane_extent(&self, group: Group, i0: usize, i1: usize) -> (f64, f64) {
        if group == Group::Pct {
            return (0.0, 100.0);
        }
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        let mut take = |v: f64| {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            }
        };
        let closed_end = i1.min(self.candles.len().saturating_sub(1));
        let series = |values: &[Option<f64>], take: &mut dyn FnMut(f64)| {
            for v in values.get(i0..=closed_end).into_iter().flatten().flatten() {
                take(*v);
            }
        };
        if group == Group::Price {
            for i in i0..=i1 {
                if let Some(b) = self.bar(i) {
                    take(b.high);
                    take(b.low);
                }
            }
            if !self.hidden.contains("ema200") {
                series(&self.engine.ema200, &mut take);
            }
            if !self.hidden.contains("ribbon") {
                series(&self.engine.ema12, &mut take);
                series(&self.engine.ema25, &mut take);
            }
        }
        for (_, study) in self.visible_studies().filter(|(_, s)| s.group == group) {
            for line in &study.lines {
                series(&line.values, &mut take);
                if line.kind == LineType::Histogram {
                    take(0.0);
                }
            }
        }
        if lo > hi { (0.0, 1.0) } else { (lo, hi) }
    }

    // ------------------------------------------------------------------ render

    pub fn render(&mut self, now: i64) -> &Frame {
        self.dirty = false;
        self.last_second = now.div_euclid(1000);
        let mut frame = std::mem::take(&mut self.frame);
        frame.begin(self.width, self.height, self.dpr, self.theme.background);
        if self.width >= 40.0 && self.height >= 60.0 {
            self.layout();
            if self.total_bars() == 0 {
                let message = self.message.clone().unwrap_or_else(|| "Loading…".to_string());
                frame.text(self.width / 2.0, self.height / 2.0, message, self.theme.text, Align::Center, Baseline::Middle, self.theme.font_px, None, None);
            } else {
                self.paint(&mut frame, now);
                self.request_history_if_near_start();
            }
        }
        self.frame = frame;
        &self.frame
    }

    /// The tradoor mark and name, bottom left of the price pane. Part of the frame
    /// itself, so every capture of a chart carries it.
    fn paint_watermark(&self, f: &mut Frame, plot: Rect) {
        const MARK: f32 = 14.0;
        if plot.w < 160.0 || plot.h < 80.0 {
            return;
        }
        let (x, y) = (plot.x + 10.0, plot.bottom() - 10.0 - MARK);
        let accent = self.theme.ribbon_fast;
        f.stroke_rect(x - 2.0, y - 2.0, MARK + 4.0, MARK + 4.0, 1.0, accent.with_alpha(80));
        let line: Vec<(f32, f32)> = [(1.0, 11.0), (5.0, 6.0), (8.5, 8.5), (13.0, 2.5)].iter().map(|(dx, dy)| (x + dx, y + dy)).collect();
        f.polyline(&line, 2.0, accent.with_alpha(150));
        f.text(x + MARK + 7.0, y + MARK / 2.0, "tradoor", self.theme.text.with_alpha(170), Align::Left, Baseline::Middle, self.theme.font_px + 2.0, None, Some(plot));
    }

    fn request_history_if_near_start(&mut self) {
        let Some(first) = self.candles.first().map(|c| c.open_time) else { return };
        if self.history_asked != Some(first) && self.time.visible().0 < HISTORY_MARGIN_BARS {
            self.history_asked = Some(first);
            self.events.push(Event::NeedHistory { before: first });
        }
    }

    fn line_color(&self, line: &tradoor_core::studies::StudyLine, study_index: usize, line_index: usize) -> Color {
        line.color.and_then(Color::parse).unwrap_or(self.theme.study_palette[(study_index + line_index) % self.theme.study_palette.len()])
    }

    fn paint(&self, f: &mut Frame, now: i64) {
        let t = &self.theme;
        let n = self.total_bars();
        let (i0, i1) = self.visible_indices(1.0);
        let (from, to) = self.time.visible();
        let price = self.panes[0];
        let plot_w = price.plot.w;
        let avail = self.time_axis_top();

        // ---- guides under everything in the lower panes
        for pane in self.panes.iter().filter(|p| p.group == Group::Pct) {
            f.clip(Some(pane.plot));
            for level in [20.0, 80.0] {
                let y = pane.scale.y(level) as f32;
                f.dashed_line(0.0, y, plot_w, y, 1.0, t.border, 4.0, 4.0);
            }
        }

        // ---- price pane: the watermark first so candles pass over its mark, then
        // the ribbon under the candles, volume at the foot
        f.clip(Some(price.plot));
        self.paint_watermark(f, price.plot);
        if !self.hidden.contains("ribbon") {
            for run in &self.ribbon {
                let pts = visible_slice(&run.points, from - 1.0, to + 1.0);
                if pts.len() < 2 {
                    continue;
                }
                let upper: Vec<(f32, f32)> = pts.iter().map(|p| (self.time.x(p.logical) as f32, price.scale.y(p.fast) as f32)).collect();
                let lower: Vec<(f32, f32)> = pts.iter().map(|p| (self.time.x(p.logical) as f32, price.scale.y(p.slow) as f32)).collect();
                let color = if run.bullish { t.ribbon_fast } else { t.ribbon_slow };
                f.band(&upper, &lower, color.with_alpha(t.ribbon_fill_alpha));
            }
        }
        let dpr = f.dpr;
        let mut body_dev = (self.time.bar_spacing as f32 * dpr * 0.8).floor().max(1.0);
        if body_dev >= 3.0 && (body_dev as i32) % 2 == 0 {
            body_dev -= 1.0;
        }
        let body = body_dev / dpr;
        if !self.hidden.contains("volume") {
            let max_vol = (i0..=i1).filter_map(|i| self.bar(i)).map(|b| b.volume).fold(0.0, f64::max);
            if max_vol > 0.0 {
                let (top, h) = (price.plot.y + price.plot.h * 0.82, price.plot.h * 0.18);
                for i in i0..=i1 {
                    let Some(b) = self.bar(i) else { continue };
                    let y = top + h * (1.0 - (b.volume / max_vol) as f32);
                    let base = if b.close >= b.open { t.bull } else { t.bear };
                    let x = f.snap(self.time.x(i as f64) as f32);
                    f.crisp_rect(x - body / 2.0, y, body, price.plot.bottom() - y, base.with_alpha(if b.live { 0x25 } else { t.volume_alpha }));
                }
            }
        }
        let wick = (body_dev / 8.0).floor().max(1.0) / dpr;
        for i in i0..=i1 {
            let Some(b) = self.bar(i) else { continue };
            let s = &price.scale;
            let x = f.snap(self.time.x(i as f64) as f32);
            let base = if b.close >= b.open { t.bull } else { t.bear };
            let fill = base.with_alpha(if b.live { 0x80 } else { t.candle_alpha });
            let (yh, yl, yo, yc) = (s.y(b.high) as f32, s.y(b.low) as f32, s.y(b.open) as f32, s.y(b.close) as f32);
            let (top, bottom) = (yo.min(yc), yo.max(yc));
            if body_dev < 2.0 {
                f.vline(x, yh, yl, wick, fill);
                continue;
            }
            if top - yh > 0.0 {
                f.vline(x, yh, top, wick, fill);
            }
            if yl - bottom > 0.0 {
                f.vline(x, bottom, yl, wick, fill);
            }
            f.crisp_rect(x - body / 2.0, top, body, (bottom - top).max(1.0 / dpr), fill);
            if b.live {
                f.stroke_rect(x - body / 2.0, top, body, (bottom - top).max(1.0 / dpr), 1.0 / dpr, base);
            }
        }
        if !self.hidden.contains("ema200") {
            self.paint_series(f, &price.scale, &self.engine.ema200, i0, i1, t.ema200, 1.0, LineStyle::Solid);
        }

        // ---- studies, each in its pane
        for (index, study) in self.visible_studies() {
            let Some(pane) = self.panes.iter().find(|p| p.group == study.group) else { continue };
            f.clip(Some(pane.plot));
            for (li, line) in study.lines.iter().enumerate() {
                match (line.kind, line.style) {
                    (LineType::Histogram, _) => self.paint_histogram(f, &pane.scale, &line.values, i0, i1, body),
                    (LineType::Line, LineStyle::None) => {}
                    (LineType::Line, style) => self.paint_series(f, &pane.scale, &line.values, i0, i1, self.line_color(line, index, li), line.width, style),
                }
            }
            for seg in &study.segments {
                let (Some(l0), Some(l1)) = (time_to_logical(&self.times, seg.from as f64), time_to_logical(&self.times, seg.to as f64)) else { continue };
                let (x0, x1) = (self.time.x(l0) as f32, self.time.x(l1) as f32);
                if x1 < 0.0 || x0 > plot_w {
                    continue;
                }
                let y = pane.scale.y(seg.price) as f32;
                let color = Color::parse(seg.color).unwrap_or(t.text);
                f.hline(x0, x1, y, 1.0, color);
                f.text(x1 + 5.0, y - 3.0, seg.label, color, Align::Left, Baseline::Bottom, t.font_px - 1.0, None, Some(pane.plot));
            }
            for m in &study.markers {
                let Ok(i) = self.candles.binary_search_by_key(&m.time, |c| c.open_time) else { continue };
                if i < i0 || i > i1 {
                    continue;
                }
                let c = &self.candles[i];
                let x = self.time.x(i as f64) as f32;
                let color = match m.tone {
                    Tone::Bull => t.bull,
                    Tone::Bear => t.bear,
                    Tone::Neutral => t.text,
                };
                if m.above {
                    let y = pane.scale.y(c.high) as f32 - 8.0;
                    f.circle(x, y, 2.5, color);
                    f.text(x, y - 5.0, m.text.clone(), color, Align::Center, Baseline::Bottom, t.font_px - 1.0, None, Some(pane.plot));
                } else {
                    let y = pane.scale.y(c.low) as f32 + 8.0;
                    f.circle(x, y, 2.5, color);
                    f.text(x, y + 5.0, m.text.clone(), color, Align::Center, Baseline::Top, t.font_px - 1.0, None, Some(pane.plot));
                }
            }
        }

        // ---- ribbon crosses, last price, drawings
        f.clip(Some(price.plot));
        if !self.hidden.contains("ribbon") {
            for cross in self.engine.crosses.iter().filter(|c| c.index >= i0 && c.index <= i1) {
                let c = &self.candles[cross.index];
                let x = self.time.x(cross.index as f64) as f32;
                if cross.bull {
                    let tip = price.scale.y(c.low) as f32 + 6.0;
                    f.triangle((x, tip), (x - 5.0, tip + 8.0), (x + 5.0, tip + 8.0), t.ribbon_fast);
                    f.text(x, tip + 10.0, "Bull", t.ribbon_fast, Align::Center, Baseline::Top, t.font_px - 1.0, None, Some(price.plot));
                } else {
                    let tip = price.scale.y(c.high) as f32 - 6.0;
                    f.triangle((x, tip), (x - 5.0, tip - 8.0), (x + 5.0, tip - 8.0), t.ribbon_slow);
                    f.text(x, tip - 10.0, "Bear", t.ribbon_slow, Align::Center, Baseline::Bottom, t.font_px - 1.0, None, Some(price.plot));
                }
            }
        }
        let last = self.bar(n - 1).expect("n > 0");
        let last_color = if last.close >= last.open { t.bull } else { t.bear };
        let last_y = price.scale.y(last.close) as f32;
        f.dashed_line(0.0, last_y, plot_w, last_y, 1.0, last_color, 2.0, 2.0);

        for placed in self.drawings.iter().map(|d| (d, false)).chain(self.draft.iter().map(|d| (d, true))).filter_map(|(d, draft)| self.place(d, draft)) {
            drawings::paint(f, &placed, price.plot, t);
        }

        // ---- crosshair
        let hover = self.crosshair.filter(|(x, y)| *x >= 0.0 && *x < plot_w && *y >= 0.0 && *y < avail && !matches!(self.drag, Drag::Separator { .. }));
        let hover_bar = hover.and_then(|(x, _)| self.time.nearest_bar(f64::from(x)));
        f.clip(None);
        if let (Some((_, cy)), Some(bar)) = (hover, hover_bar) {
            let x = f.snap(self.time.x(bar as f64) as f32);
            f.dashed_line(x, 0.0, x, avail, 1.0, t.crosshair, 4.0, 4.0);
            if self.pane_at(cy).is_some() {
                f.dashed_line(0.0, cy, plot_w, cy, 1.0, t.crosshair, 4.0, 4.0);
            }
        }

        // ---- chrome
        f.vline(plot_w, 0.0, avail, 1.0, t.border);
        f.hline(0.0, self.width, avail, 1.0, t.border);
        for pane in &self.panes[1..] {
            f.hline(0.0, self.width, pane.plot.y, 1.0, t.border);
        }

        // ---- badges: last values, countdown, crosshair price — placed first so
        // tick labels can step out of their way
        let font = t.font_px;
        let badge_h = font + 6.0;
        struct Badge {
            pane: usize,
            y: f32,
            text: String,
            bg: Color,
            fg: Color,
        }
        let mut badges: Vec<Badge> = Vec::new();
        let p = self.precision();
        badges.push(Badge { pane: 0, y: last_y, text: format_price(last.close, Some(p)), bg: last_color, fg: last_color.contrast_text() });
        if self.timeframe.source().interval.ms().is_some() || self.timeframe.source().interval == Interval::Mo1 {
            let close_at = self.timeframe.end(self.timeframe.align(now));
            badges.push(Badge { pane: 0, y: last_y + badge_h, text: format_countdown(close_at - now), bg: t.countdown_background, fg: t.countdown_text });
        }
        let last_value = |values: &[Option<f64>]| values.iter().rev().flatten().next().copied();
        if !self.hidden.contains("ema200") {
            if let Some(v) = last_value(&self.engine.ema200) {
                badges.push(Badge { pane: 0, y: price.scale.y(v) as f32, text: format_price(v, Some(p)), bg: t.ema200, fg: t.ema200.contrast_text() });
            }
        }
        for (index, study) in self.visible_studies() {
            let Some(pane_index) = self.panes.iter().position(|pn| pn.group == study.group) else { continue };
            let pane = self.panes[pane_index];
            let decimals = if study.group == Group::Price { p } else { 2 };
            for (li, line) in study.lines.iter().enumerate().filter(|(_, l)| l.kind == LineType::Line) {
                if let Some(v) = last_value(&line.values) {
                    let color = self.line_color(line, index, li);
                    badges.push(Badge { pane: pane_index, y: pane.scale.y(v) as f32, text: format_price(v, Some(decimals)), bg: color, fg: color.contrast_text() });
                }
            }
        }
        badges.sort_by(|a, b| a.pane.cmp(&b.pane).then(a.y.total_cmp(&b.y)));
        let mut previous: Option<(usize, f32)> = None;
        for badge in &mut badges {
            if let Some((pane, y)) = previous.filter(|(pane, _)| *pane == badge.pane) {
                let _ = pane;
                badge.y = badge.y.max(y + badge_h);
            }
            previous = Some((badge.pane, badge.y));
        }
        let crosshair_price = hover.and_then(|(_, cy)| self.pane_at(cy).map(|i| (i, cy)));

        // ---- price tick labels, skipping any a badge sits on
        for (pi, pane) in self.panes.iter().enumerate() {
            let axis = Rect { x: plot_w, y: pane.plot.y, w: self.axis_width, h: pane.plot.h };
            let ticks = ticks::price_ticks(&pane.scale, 44.0);
            let decimals = match pane.group {
                Group::Price => p,
                Group::Pct => 2,
                _ => ticks::step_decimals(&ticks).max(2),
            };
            for v in ticks {
                let y = pane.scale.y(v) as f32;
                let near_badge = badges.iter().any(|b| b.pane == pi && (b.y - y).abs() < badge_h);
                let near_cross = crosshair_price.is_some_and(|(i, cy)| i == pi && (cy - y).abs() < badge_h);
                if y < pane.plot.y + 6.0 || y > pane.plot.bottom() - 6.0 || near_badge || near_cross {
                    continue;
                }
                f.text(plot_w + 6.0, y, format_price(v, Some(decimals)), t.text, Align::Left, Baseline::Middle, font, None, Some(axis));
            }
            if pane.group == Group::Pct {
                for level in [20.0, 80.0] {
                    let y = pane.scale.y(level) as f32;
                    f.crisp_rect(plot_w + 1.0, y - badge_h / 2.0, self.axis_width - 1.0, badge_h, t.border);
                    f.text(plot_w + 6.0, y, format_price(level, Some(2)), t.text, Align::Left, Baseline::Middle, font, None, Some(axis));
                }
            }
        }
        for badge in &badges {
            let pane = self.panes[badge.pane];
            if badge.y < pane.plot.y || badge.y > pane.plot.bottom() {
                continue;
            }
            let axis = Rect { x: plot_w, y: pane.plot.y, w: self.axis_width, h: pane.plot.h };
            f.crisp_rect(plot_w + 1.0, badge.y - badge_h / 2.0, self.axis_width - 1.0, badge_h, badge.bg);
            f.text(plot_w + 6.0, badge.y, badge.text.clone(), badge.fg, Align::Left, Baseline::Middle, font, None, Some(axis));
        }
        if let Some((pi, cy)) = crosshair_price {
            let pane = self.panes[pi];
            let decimals = if pane.group == Group::Price { p } else { 2 };
            f.crisp_rect(plot_w + 1.0, cy - badge_h / 2.0, self.axis_width - 1.0, badge_h, t.label_background);
            f.text(plot_w + 6.0, cy, format_price(pane.scale.price(f64::from(cy)), Some(decimals)), t.countdown_text, Align::Left, Baseline::Middle, font, None, None);
        }

        // ---- time labels
        let time_axis = Rect { x: 0.0, y: avail, w: plot_w, h: TIME_AXIS_HEIGHT };
        let cross_label = hover_bar.and_then(|i| self.bar(i)).map(|b| {
            let monthly = self.timeframe.source().interval == Interval::Mo1;
            let text = ticks::crosshair_label(b.time, self.timeframe.source().interval.ms(), monthly);
            let w = text.len() as f32 * self.char_width + 12.0;
            (self.time.x(hover_bar.unwrap_or(0) as f64) as f32, text, w)
        });
        let mut candidates = Vec::new();
        for i in i0..=i1 {
            let Some(b) = self.bar(i) else { continue };
            let x = self.time.x(i as f64) as f32;
            if (0.0..plot_w).contains(&x) {
                let prev = i.checked_sub(1).and_then(|j| self.bar(j)).map(|pb| pb.time);
                candidates.push((ticks::boundary_weight(prev, b.time), x, b.time));
            }
        }
        for (x, label) in ticks::pick_time_labels(&candidates, self.char_width * 6.0 + 22.0) {
            if cross_label.as_ref().is_some_and(|(cx, _, w)| (cx - x).abs() < w / 2.0 + self.char_width * 3.0) {
                continue;
            }
            f.text(x, avail + TIME_AXIS_HEIGHT / 2.0, label, t.text, Align::Center, Baseline::Middle, font, None, Some(time_axis));
        }
        if let Some((x, text, w)) = cross_label {
            let left = (x - w / 2.0).clamp(0.0, (plot_w - w).max(0.0));
            f.crisp_rect(left, avail + 1.0, w, TIME_AXIS_HEIGHT - 2.0, t.label_background);
            f.text(left + w / 2.0, avail + TIME_AXIS_HEIGHT / 2.0, text, t.countdown_text, Align::Center, Baseline::Middle, font, None, None);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_series(&self, f: &mut Frame, scale: &PriceScale, values: &[Option<f64>], i0: usize, i1: usize, color: Color, width: f32, style: LineStyle) {
        if values.is_empty() {
            return;
        }
        let end = i1.min(values.len() - 1);
        let mut pts: Vec<(f32, f32)> = Vec::new();
        for i in i0..=end {
            match values[i] {
                Some(v) => {
                    let (x, y) = (self.time.x(i as f64) as f32, scale.y(v) as f32);
                    if let (LineStyle::Step, Some(&(_, py))) = (style, pts.last()) {
                        pts.push((x, py));
                    }
                    pts.push((x, y));
                }
                None => {
                    f.polyline(&pts, width, color);
                    pts.clear();
                }
            }
        }
        f.polyline(&pts, width, color);
    }

    fn paint_histogram(&self, f: &mut Frame, scale: &PriceScale, values: &[Option<f64>], i0: usize, i1: usize, body: f32) {
        let zero = scale.y(0.0) as f32;
        let w = (body * 0.7).max(1.0 / f.dpr);
        for i in i0..=i1.min(values.len().saturating_sub(1)) {
            let Some(v) = values[i] else { continue };
            let y = scale.y(v) as f32;
            let x = f.snap(self.time.x(i as f64) as f32);
            let color = if v >= 0.0 { self.theme.bull } else { self.theme.bear };
            f.crisp_rect(x - w / 2.0, y.min(zero), w, (y - zero).abs().max(1.0 / f.dpr), color.with_alpha(0x80));
        }
    }

    fn place<'a>(&self, shape: &'a DrawingShape, draft: bool) -> Option<Placed<'a>> {
        let pane = self.panes.first()?;
        let mut points = Vec::with_capacity(shape.points.len());
        for a in &shape.points {
            let x = self.time.x(time_to_logical(&self.times, a.time)?) as f32;
            let y = pane.scale.y(a.price) as f32;
            if !(x.is_finite() && y.is_finite()) {
                return None;
            }
            points.push((x, y));
        }
        let bars = match (shape.points.first(), shape.points.get(1)) {
            (Some(a), Some(b)) => (time_to_logical(&self.times, b.time)? - time_to_logical(&self.times, a.time)?).round() as i64,
            _ => 0,
        };
        Some(Placed { shape, points, selected: !draft && shape.id.is_some() && shape.id == self.selected, draft, bars })
    }

    // ------------------------------------------------------------------ legend

    pub fn legend(&self) -> Legend {
        let n = self.total_bars();
        let Some(pane) = self.panes.first() else { return Legend::default() };
        if n == 0 {
            return Legend::default();
        }
        let hovered = self.crosshair.filter(|(x, y)| *x >= 0.0 && *x < pane.plot.w && *y >= 0.0 && *y < self.time_axis_top()).and_then(|(x, _)| self.time.nearest_bar(f64::from(x)));
        let index = hovered.unwrap_or(n - 1);
        let Some(b) = self.bar(index) else { return Legend::default() };
        let p = self.precision();
        let fp = |v: f64| format_price(v, Some(p));
        let diff = b.close - b.open;
        let pct = if b.open == 0.0 { 0.0 } else { diff / b.open * 100.0 };
        let sign = if diff > 0.0 { "+" } else { "" };
        // Indicators stop at the last closed bar; hovering the forming one reads that.
        let at = index.min(self.candles.len().saturating_sub(1));
        let values = self
            .visible_studies()
            .flat_map(|(_, s)| {
                s.lines.iter().map(move |l| {
                    let decimals = if s.group == Group::Price { p } else { 2 };
                    (format!("{}:{}", s.id, l.key), l.values.get(at).copied().flatten().map_or("—".to_string(), |v| format_price(v, Some(decimals))))
                })
            })
            .collect();
        Legend {
            index: Some(index),
            time: b.time,
            ohlc: format!("O{}  H{}  L{}  C{}", fp(b.open), fp(b.high), fp(b.low), fp(b.close)),
            change: format!("{sign}{} ({sign}{pct:.2}%)", fp(diff)),
            up: diff >= 0.0,
            volume: format!("Vol {}", format_turnover(b.volume)),
            values,
        }
    }

    pub fn studies_meta(&self) -> Vec<StudyMeta> {
        let mut visible = 0usize;
        self.studies
            .iter()
            .map(|s| {
                let hidden = self.hidden.contains(&s.id);
                let index = visible;
                if !hidden {
                    visible += 1;
                }
                StudyMeta {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    group: s.group.as_str(),
                    hidden,
                    lines: s.lines.iter().enumerate().map(|(li, l)| (format!("{}:{}", s.id, l.key), l.label.clone(), self.line_color(l, index, li))).collect(),
                    stats: s.stats.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
                }
            })
            .collect()
    }

    // ------------------------------------------------------------------ interaction

    fn pane_at(&self, y: f32) -> Option<usize> {
        self.panes.iter().position(|p| y >= p.plot.y && y < p.plot.bottom())
    }

    fn zone(&self, x: f32, y: f32) -> Zone {
        if self.panes.is_empty() || x < 0.0 || y < 0.0 || x > self.width || y > self.height {
            return Zone::Outside;
        }
        if let Some(i) = (1..self.panes.len()).find(|&i| (y - self.panes[i].plot.y).abs() <= SEPARATOR_GRAB) {
            return Zone::Separator(i - 1);
        }
        let plot_w = self.plot_width();
        if y >= self.time_axis_top() {
            return if x < plot_w { Zone::TimeAxis } else { Zone::Outside };
        }
        match self.pane_at(y) {
            Some(p) if x < plot_w => Zone::Plot(p),
            Some(p) => Zone::PriceAxis(p),
            None => Zone::Outside,
        }
    }

    fn anchor_at(&self, x: f32, y: f32) -> Option<Anchor> {
        let pane = self.panes.first()?;
        let time = logical_to_time(&self.times, self.time.logical(f64::from(x)))?;
        let price = pane.scale.price(f64::from(y));
        (time.is_finite() && price.is_finite()).then_some(Anchor { time, price })
    }

    fn drawing_at(&self, x: f32, y: f32) -> Option<i64> {
        let (mut best, mut best_distance) = (None, HIT_TOLERANCE);
        for d in &self.drawings {
            let Some(placed) = self.place(d, false) else { continue };
            let distance = distance_to_drawing(d.kind.as_str(), &placed.points, (x, y), &FIB_LEVELS);
            // `<=`: later drawings are painted on top, so the visible one wins a tie.
            if distance <= best_distance {
                best_distance = distance;
                best = d.id;
            }
        }
        best
    }

    fn cursor_for(&self, x: f32, y: f32) -> Cursor {
        if self.tool.is_some() {
            return Cursor::Crosshair;
        }
        match self.drag {
            Drag::Press { panning: true, .. } => Cursor::Grabbing,
            Drag::PriceAxis { .. } => Cursor::NsResize,
            Drag::TimeAxis { .. } => Cursor::ColResize,
            Drag::Separator { .. } => Cursor::RowResize,
            _ => match self.zone(x, y) {
                Zone::Separator(_) => Cursor::RowResize,
                Zone::PriceAxis(_) => Cursor::NsResize,
                Zone::TimeAxis => Cursor::ColResize,
                Zone::Plot(0) if self.drawing_at(x, y).is_some() => Cursor::Pointer,
                Zone::Plot(_) => Cursor::Crosshair,
                Zone::Outside => Cursor::Default,
            },
        }
    }

    fn draft_with(&mut self, kind: DrawingKind, extra: Option<Anchor>) {
        let mut points = self.placed.clone();
        points.extend(extra);
        self.draft = Some(DrawingShape { id: None, kind, points, label: None });
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.crosshair = Some((x, y));
        match self.drag {
            Drag::Press { start, last_x, last_y, pane, panning, price_pan } => {
                if self.tool.is_none() && (panning || (x - start.0).hypot(y - start.1) > CLICK_SLOP) {
                    self.time.pan(f64::from(x - last_x));
                    // Price follows too: at once on a pane already scaled by hand,
                    // and on an autoscaled one when the drag is clearly vertical.
                    let follow = price_pan || !self.pane_auto(pane) || (y - start.1).abs() > VERTICAL_PAN_SLOP;
                    if follow {
                        // The first step covers the whole drag so far, keeping the
                        // grabbed price under the pointer.
                        self.pan_price(pane, if price_pan { y - last_y } else { y - start.1 });
                    }
                    self.drag = Drag::Press { start, last_x: x, last_y: y, pane, panning: true, price_pan: follow };
                }
            }
            Drag::PriceAxis { pane, last_y } => {
                self.zoom_price(pane, (f64::from(y - last_y) * 0.004).exp());
                self.drag = Drag::PriceAxis { pane, last_y: y };
            }
            Drag::TimeAxis { last_x } => {
                let right = self.time.width;
                self.time.zoom_at(right, (f64::from(x - last_x) * 0.01).exp());
                self.drag = Drag::TimeAxis { last_x: x };
            }
            Drag::Separator { upper, last_y } => {
                self.resize_panes(upper, y - last_y);
                self.drag = Drag::Separator { upper, last_y: y };
            }
            Drag::Brush { last } => {
                if (x - last.0).hypot(y - last.1) >= BRUSH_SAMPLE && self.placed.len() < MAX_POINTS {
                    if let Some(a) = self.anchor_at(x, y) {
                        self.placed.push(a);
                        self.draft_with(DrawingKind::Brush, None);
                        self.drag = Drag::Brush { last: (x, y) };
                    }
                }
            }
            Drag::None => {}
        }
        if let Some(tool) = self.tool.filter(|t| *t != DrawingKind::Brush && !self.placed.is_empty()) {
            if let Some(a) = self.anchor_at(x, y) {
                self.draft_with(tool, Some(a));
            }
        }
        self.cursor = self.cursor_for(x, y);
        self.touch();
    }

    pub fn pointer_down(&mut self, x: f32, y: f32, button: u16) {
        if button != 0 {
            return;
        }
        let free = self.tool.is_none();
        self.drag = match self.zone(x, y) {
            Zone::Separator(i) if free => Drag::Separator { upper: i, last_y: y },
            Zone::PriceAxis(p) if free => Drag::PriceAxis { pane: p, last_y: y },
            Zone::TimeAxis if free => Drag::TimeAxis { last_x: x },
            Zone::Plot(0) if self.tool == Some(DrawingKind::Brush) => match self.anchor_at(x, y) {
                Some(a) => {
                    self.placed = vec![a];
                    self.draft_with(DrawingKind::Brush, None);
                    Drag::Brush { last: (x, y) }
                }
                None => Drag::None,
            },
            Zone::Plot(p) => Drag::Press { start: (x, y), last_x: x, last_y: y, pane: p, panning: false, price_pan: false },
            _ => Drag::None,
        };
        self.cursor = self.cursor_for(x, y);
        self.touch();
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) {
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Press { start, panning: false, .. } if (x - start.0).hypot(y - start.1) <= CLICK_SLOP => self.click(x, y),
            Drag::Brush { .. } => {
                let points = std::mem::take(&mut self.placed);
                self.draft = None;
                // A press with no movement is a dot, not a stroke.
                if points.len() >= DrawingKind::Brush.arity() {
                    self.events.push(Event::DrawingCreated { kind: DrawingKind::Brush, points });
                    self.finish_tool();
                }
            }
            Drag::Separator { .. } => self.events.push(Event::PaneWeightsChanged(self.pane_weights())),
            _ => {}
        }
        self.cursor = self.cursor_for(x, y);
        self.touch();
    }

    pub fn pointer_leave(&mut self) {
        self.crosshair = None;
        if !matches!(self.drag, Drag::Brush { .. }) {
            self.drag = Drag::None;
        }
        self.touch();
    }

    fn finish_tool(&mut self) {
        self.tool = None;
        self.placed.clear();
        self.draft = None;
        self.events.push(Event::ToolFinished);
    }

    fn click(&mut self, x: f32, y: f32) {
        let on_price = self.zone(x, y) == Zone::Plot(0);
        match self.tool {
            None => {
                let hit = if on_price { self.drawing_at(x, y) } else { None };
                if hit != self.selected {
                    self.selected = hit;
                    self.events.push(Event::SelectionChanged(hit));
                }
            }
            Some(DrawingKind::Brush) => {}
            Some(tool) => {
                let Some(anchor) = self.anchor_at(x, y).filter(|_| on_price) else { return };
                self.placed.push(anchor);
                if tool.open_ended() || self.placed.len() < tool.arity() {
                    // The draft repeats the newest anchor so it has something to
                    // rubber-band from before the pointer moves.
                    self.draft_with(tool, Some(anchor));
                    return;
                }
                let points = std::mem::take(&mut self.placed);
                self.events.push(Event::DrawingCreated { kind: tool, points });
                self.finish_tool();
            }
        }
    }

    fn finish_path(&mut self) -> bool {
        // A double-click reports both clicks first, so the last anchor arrives twice.
        let mut points = std::mem::take(&mut self.placed);
        points.dedup_by(|a, b| a.time == b.time && a.price == b.price);
        if points.len() < DrawingKind::Path.arity() {
            self.placed = points;
            return false;
        }
        self.events.push(Event::DrawingCreated { kind: DrawingKind::Path, points });
        self.finish_tool();
        true
    }

    pub fn double_click(&mut self, x: f32, y: f32) {
        if self.tool == Some(DrawingKind::Path) {
            self.finish_path();
        } else {
            match self.zone(x, y) {
                Zone::PriceAxis(p) => {
                    if let Some(pane) = self.panes.get(p) {
                        let group = pane.group;
                        let was = self.auto_scale();
                        self.pane_state.insert(group, PaneState { weight: self.weight(group), ..PaneState::default_for(group) });
                        self.note_auto(was);
                    }
                }
                Zone::TimeAxis => self.view_ready = false,
                _ => {}
            }
        }
        self.touch();
    }

    /// Returns whether the wheel was used, so the page knows not to scroll.
    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        if self.tool.is_some() {
            return false;
        }
        let used = match self.zone(x, y) {
            Zone::PriceAxis(p) => {
                let group = self.panes[p].group;
                let state = self.pane_state.entry(group).or_insert_with(|| PaneState::default_for(group));
                let step = if dy > 0.0 { MARGIN_STEP } else { -MARGIN_STEP };
                state.margins = ((state.margins.0 + step).clamp(0.0, MAX_MARGIN), (state.margins.1 + step).clamp(0.0, MAX_MARGIN));
                true
            }
            Zone::Plot(_) | Zone::TimeAxis => {
                if dx.abs() > dy.abs() {
                    self.time.pan(f64::from(-dx));
                } else {
                    // With the newest bar on screen the right edge holds still, so
                    // zooming out brings in history rather than empty space after
                    // the last bar. Scrolled back, the bar under the pointer holds.
                    let at = if self.time.right_offset >= 0.0 { self.plot_width() } else { x.min(self.plot_width()) };
                    self.time.zoom_at(f64::from(at), (-f64::from(dy) * 0.002).exp());
                }
                true
            }
            _ => false,
        };
        if used {
            self.touch();
        }
        used
    }

    /// Escape, Enter and Delete, when they mean something to the chart. Returns
    /// whether the key was used, so an unused Escape can still navigate.
    pub fn key(&mut self, key: &str) -> bool {
        let used = match key {
            "Escape" if !self.placed.is_empty() => {
                self.placed.clear();
                self.draft = None;
                true
            }
            "Escape" if self.tool.is_some() => {
                self.finish_tool();
                true
            }
            "Escape" if self.selected.is_some() => {
                self.selected = None;
                self.events.push(Event::SelectionChanged(None));
                true
            }
            "Enter" if self.tool == Some(DrawingKind::Path) => self.finish_path(),
            "Delete" | "Backspace" => match self.selected {
                Some(id) => {
                    self.events.push(Event::DeleteRequested(id));
                    true
                }
                None => false,
            },
            _ => false,
        };
        if used {
            self.touch();
        }
        used
    }

    /// Whether every pane's price axis follows the bars on screen.
    pub fn auto_scale(&self) -> bool {
        self.pane_state.values().all(|s| s.auto)
    }

    /// Every pane back to following the bars on screen, keeping heights and margins.
    pub fn set_auto_scale(&mut self) {
        let was = self.auto_scale();
        for state in self.pane_state.values_mut() {
            state.auto = true;
            state.manual = None;
        }
        self.note_auto(was);
        self.touch();
    }

    fn note_auto(&mut self, was: bool) {
        let now = self.auto_scale();
        if now != was {
            self.events.push(Event::AutoScaleChanged(now));
        }
    }

    fn pane_auto(&self, pane: usize) -> bool {
        self.panes.get(pane).is_none_or(|v| self.pane_state.get(&v.group).is_none_or(|s| s.auto))
    }

    /// A range set by hand: the pane stops following the bars on screen.
    fn set_manual(&mut self, pane: usize, change: impl FnOnce(&mut PriceScale)) {
        let Some(view) = self.panes.get_mut(pane) else { return };
        change(&mut view.scale);
        let (range, group) = (view.scale.range, view.group);
        let was = self.auto_scale();
        let state = self.pane_state.entry(group).or_insert_with(|| PaneState::default_for(group));
        state.auto = false;
        state.manual = Some(range);
        self.note_auto(was);
        self.touch();
    }

    fn zoom_price(&mut self, pane: usize, factor: f64) {
        self.set_manual(pane, |scale| scale.zoom(factor));
    }

    fn pan_price(&mut self, pane: usize, dy: f32) {
        self.set_manual(pane, |scale| scale.pan(f64::from(dy)));
    }

    fn resize_panes(&mut self, upper: usize, dy: f32) {
        let (Some(a), Some(b)) = (self.panes.get(upper).copied(), self.panes.get(upper + 1).copied()) else { return };
        let (ha, hb) = (a.plot.h + dy, b.plot.h - dy);
        if ha < MIN_PANE_HEIGHT || hb < MIN_PANE_HEIGHT {
            return;
        }
        let total: f64 = self.panes.iter().map(|p| self.weight(p.group)).sum();
        let avail = f64::from(self.time_axis_top());
        for (group, h) in [(a.group, ha), (b.group, hb)] {
            self.pane_state.entry(group).or_insert_with(|| PaneState::default_for(group)).weight = f64::from(h) / avail * total;
        }
        self.panes[upper].plot.h = ha;
        self.panes[upper + 1].plot.y += dy;
        self.panes[upper + 1].plot.h = hb;
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tradoor_core::time::HOUR;

    fn candles(n: usize) -> Vec<Candle> {
        (0..n)
            .map(|i| {
                let p = 100.0 + (i as f64 / 7.0).sin() * 10.0 + i as f64 * 0.05;
                Candle { open_time: 1_700_000_000_000 - 1_700_000_000_000 % HOUR + i as i64 * HOUR, open: p - 0.5, high: p + 1.5, low: p - 2.0, close: p + 0.5, volume: 10.0 + i as f64, quote_volume: None }
            })
            .collect()
    }

    fn chart() -> Chart {
        let mut c = Chart::new(ChartTimeframe::H1);
        c.resize(1000.0, 600.0, 1.0);
        c.set_candles(candles(500));
        c.render(1_800_000_000_000);
        c
    }

    fn click(c: &mut Chart, x: f32, y: f32) {
        c.pointer_down(x, y, 0);
        c.pointer_up(x, y);
    }

    #[test]
    fn empty_chart_says_so() {
        let mut c = Chart::new(ChartTimeframe::D1);
        c.resize(800.0, 400.0, 2.0);
        c.set_message(Some("no data".into()));
        let frame = c.render(0);
        assert_eq!(frame.texts.len(), 1);
        assert_eq!(frame.texts[0].text, "no data");
    }

    #[test]
    fn a_chart_draws_candles_axes_and_badges() {
        let mut c = chart();
        let frame = c.render(1_800_000_000_000);
        assert!(frame.vertex_count() > 150 * 6, "candles, wicks and volume");
        assert!(frame.texts.iter().any(|t| t.text.contains(':')), "time labels");
        assert!(frame.positions.iter().all(|v| v.is_finite()));
        assert_eq!(frame.colors.len(), frame.positions.len() * 2);
        let legend = c.legend();
        assert_eq!(legend.index, Some(499));
        assert!(legend.ohlc.starts_with('O'));
    }

    #[test]
    fn wheel_zoom_holds_the_newest_bar_or_else_the_bar_under_the_pointer() {
        let mut c = chart();
        let offset = c.time.right_offset;
        assert!(c.wheel(400.0, 200.0, 0.0, 240.0));
        assert!((c.time.right_offset - offset).abs() < 1e-9, "zooming out keeps the last bar at the right edge");

        c.time.pan(100.0 * c.time.bar_spacing);
        assert!(c.time.right_offset < 0.0);
        let before = c.time.logical(400.0);
        assert!(c.wheel(400.0, 200.0, 0.0, -120.0));
        assert!((c.time.logical(400.0) - before).abs() < 1e-6);
    }

    #[test]
    fn dragging_up_or_down_moves_price_and_turns_autoscale_off() {
        let mut c = chart();
        let grabbed = c.panes[0].scale.price(200.0);
        c.pointer_down(500.0, 200.0, 0);
        c.pointer_move(505.0, 204.0);
        c.pointer_move(510.0, 210.0);
        assert!(c.auto_scale(), "a sideways pan keeps autoscale");
        c.pointer_move(510.0, 260.0);
        c.pointer_up(510.0, 260.0);
        assert!(!c.auto_scale());
        assert!((c.panes[0].scale.price(260.0) - grabbed).abs() < 1e-6, "the grabbed price stays under the pointer");
        assert!(c.take_events().contains(&Event::AutoScaleChanged(false)));
        let range = c.panes[0].scale.range;
        c.render(1_800_000_000_000);
        assert_eq!(c.panes[0].scale.range, range, "a range set by hand survives a frame");

        c.set_auto_scale();
        assert!(c.auto_scale());
        assert_eq!(c.take_events(), vec![Event::AutoScaleChanged(true)]);

        c.pointer_down(500.0, 200.0, 0);
        c.pointer_move(500.0, 300.0);
        c.pointer_up(500.0, 300.0);
        assert!(!c.auto_scale());
        c.set_candles(Vec::new());
        c.render(1_800_000_000_000);
        c.set_candles(candles(300));
        c.render(1_800_000_000_000);
        assert!(c.auto_scale(), "new bars start autoscaled");
    }

    #[test]
    fn dragging_the_plot_pans() {
        let mut c = chart();
        let offset = c.time.right_offset;
        c.pointer_down(500.0, 200.0, 0);
        c.pointer_move(520.0, 200.0);
        c.pointer_move(600.0, 200.0);
        c.pointer_up(600.0, 200.0);
        assert!(c.time.right_offset < offset, "dragging right reveals older bars");
        assert!(c.take_events().iter().all(|e| !matches!(e, Event::SelectionChanged(_))), "a pan is not a click");
    }

    #[test]
    fn two_clicks_place_a_trendline_at_the_prices_clicked() {
        let mut c = chart();
        c.set_tool(Some(DrawingKind::Trendline));
        click(&mut c, 300.0, 200.0);
        c.pointer_move(500.0, 150.0);
        click(&mut c, 500.0, 150.0);
        let events = c.take_events();
        let Event::DrawingCreated { kind, points } = &events[0] else { panic!("{events:?}") };
        assert_eq!((*kind, points.len()), (DrawingKind::Trendline, 2));
        assert!((c.panes[0].scale.y(points[1].price) - 150.0).abs() < 1e-6);
        assert_eq!(events[1], Event::ToolFinished);
        assert_eq!(c.tool(), None);
    }

    #[test]
    fn clicking_a_level_selects_it_and_delete_asks_to_remove_it() {
        let mut c = chart();
        let price = c.panes[0].scale.price(180.0);
        c.set_drawings(vec![DrawingShape { id: Some(7), kind: DrawingKind::Horizontal, points: vec![Anchor { time: c.times[10], price }], label: None }]);
        c.render(1_800_000_000_000);
        click(&mut c, 50.0, 183.0);
        assert_eq!(c.take_events(), vec![Event::SelectionChanged(Some(7))]);
        assert!(c.key("Delete"));
        assert_eq!(c.take_events(), vec![Event::DeleteRequested(7)]);
        assert!(c.key("Escape"));
        assert!(!c.key("Escape"), "nothing left to cancel");
    }

    #[test]
    fn a_path_finishes_on_enter_and_the_brush_on_release() {
        let mut c = chart();
        c.set_tool(Some(DrawingKind::Path));
        for x in [200.0, 300.0, 400.0] {
            click(&mut c, x, 200.0);
        }
        assert!(c.key("Enter"));
        assert!(matches!(&c.take_events()[0], Event::DrawingCreated { kind: DrawingKind::Path, points } if points.len() == 3));

        c.set_tool(Some(DrawingKind::Brush));
        c.pointer_down(100.0, 100.0, 0);
        for i in 1..20 {
            c.pointer_move(100.0 + i as f32 * 5.0, 100.0 + i as f32);
        }
        c.pointer_up(200.0, 120.0);
        assert!(matches!(&c.take_events()[0], Event::DrawingCreated { kind: DrawingKind::Brush, points } if points.len() > 10));
    }

    #[test]
    fn the_watermark_sits_bottom_left_of_the_price_pane() {
        let mut c = chart();
        c.set_studies("rsi:14");
        let texts = c.render(1_800_000_000_000).texts.clone();
        let price = c.panes[0].plot;
        let mark = texts.iter().find(|t| t.text == "tradoor").expect("watermark text");
        assert!(mark.x < price.x + 60.0, "left");
        assert!(mark.y > price.bottom() - 30.0 && mark.y < price.bottom(), "bottom of the price pane, above the lower panes");
    }

    #[test]
    fn studies_open_their_own_panes() {
        let mut c = chart();
        c.set_studies("rsi:14,stochrsi,macd,ema:50");
        c.render(1_800_000_000_000);
        assert_eq!(c.panes.iter().map(|p| p.group).collect::<Vec<_>>(), [Group::Price, Group::Pct, Group::Macd]);
        let meta = c.studies_meta();
        assert_eq!(meta.len(), 4);
        assert_eq!(meta[2].lines.len(), 3);
        c.set_hidden("macd:12:26:9");
        c.render(1_800_000_000_000);
        assert_eq!(c.panes.len(), 2);
        assert!(c.legend().values.iter().any(|(id, _)| id == "rsi:14:rsi"));
    }

    #[test]
    fn scrolling_back_asks_for_history_once() {
        let mut c = chart();
        assert!(c.take_events().is_empty());
        c.time.pan(1e9);
        c.render(1_800_000_000_000);
        c.render(1_800_000_000_000);
        let asks = c.take_events().into_iter().filter(|e| matches!(e, Event::NeedHistory { .. })).count();
        assert_eq!(asks, 1);
    }

    #[test]
    fn log_scale_and_live_bar() {
        let mut c = chart();
        c.set_log_scale(true);
        let last = *c.candles().last().unwrap();
        c.set_live(Some(LiveBar { open_time: last.open_time + HOUR, open: 120.0, high: 125.0, low: 119.0, close: 124.0, volume: 3.0, closed: false }));
        let frame = c.render(1_800_000_000_000);
        assert!(frame.vertex_count() > 0);
        assert!(c.panes[0].scale.y(125.0) < c.panes[0].scale.y(119.0));
        assert_eq!(c.legend().index, Some(500));
        c.set_live(Some(LiveBar { open_time: last.open_time, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0, closed: false }));
        assert!(c.live.is_none(), "a bar that is already stored is not forming");
    }

    #[test]
    fn separators_resize_panes() {
        let mut c = chart();
        c.set_studies("rsi:14");
        c.render(1_800_000_000_000);
        let y = c.panes[1].plot.y;
        let before = c.panes[1].plot.h;
        c.pointer_down(300.0, y, 0);
        c.pointer_move(300.0, y - 40.0);
        c.pointer_up(300.0, y - 40.0);
        c.render(1_800_000_000_000);
        assert!((c.panes[1].plot.h - (before + 40.0)).abs() < 2.0);
        assert!(c.take_events().iter().any(|e| matches!(e, Event::PaneWeightsChanged(_))));
    }
}
