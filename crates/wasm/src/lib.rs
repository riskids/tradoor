//! tradoor-wasm — what the browser worker calls.
//!
//! Numbers cross the boundary as typed arrays and plain values, never as JSON:
//! the browser's own `JSON.parse` reads exchange messages, and this side
//! receives only what it needs. The chart itself lives in `tradoor-render`;
//! this crate uploads its frames.

mod gl;
mod js;
mod text;

use js_sys::{Array, Object};
use tradoor_codec::TickCandles;
use tradoor_core::chart::{engine_series, profile_overlay, profile_spec_for};
use tradoor_core::indicators::profile::ProfileBasis;
use tradoor_core::indicators::{rsi, stoch_rsi};
use tradoor_core::live::{Kline, LiveBar, LiveFeed};
use tradoor_core::studies::STUDIES;
use tradoor_core::timeframe::fold;
use tradoor_core::{ChartTimeframe, Interval};
use tradoor_render::{Anchor, Chart, DrawingKind, DrawingShape, Event};
use wasm_bindgen::prelude::*;

use crate::js::{candles, columns, get, obj, opt, set};

#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&JsValue::from_str(&format!("tradoor engine panic: {info}")))));
}

fn timeframe(tf: &str) -> Result<ChartTimeframe, JsValue> {
    ChartTimeframe::parse(tf).ok_or_else(|| JsValue::from_str(&format!("unknown timeframe: {tf}")))
}

#[wasm_bindgen]
pub struct ChartEngine {
    chart: Chart,
    gl: gl::Gl,
    text: text::TextLayer,
}

#[wasm_bindgen]
impl ChartEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(gl_canvas: web_sys::OffscreenCanvas, text_canvas: web_sys::OffscreenCanvas, tf: &str) -> Result<ChartEngine, JsValue> {
        let mut chart = Chart::new(timeframe(tf)?);
        let text = text::TextLayer::new(text_canvas)?;
        chart.set_char_width(text.char_width(chart.theme().font_px));
        Ok(Self { chart, gl: gl::Gl::new(gl_canvas)?, text })
    }

    pub fn resize(&mut self, width: f32, height: f32, dpr: f32) {
        self.chart.resize(width, height, dpr);
    }

    pub fn set_timeframe(&mut self, tf: &str) -> Result<(), JsValue> {
        self.chart.set_timeframe(timeframe(tf)?);
        Ok(())
    }

    pub fn set_candles(&mut self, time: &[f64], open: &[f64], high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) {
        self.chart.set_candles(candles(time, open, high, low, close, volume));
    }

    pub fn set_live(&mut self, open_time: f64, open: f64, high: f64, low: f64, close: f64, volume: f64) {
        self.chart.set_live(Some(LiveBar { open_time: open_time as i64, open, high, low, close, volume, closed: false }));
    }

    pub fn clear_live(&mut self) {
        self.chart.set_live(None);
    }

    /// One liquidation to ping beside the candles: `at` is when it reached us.
    pub fn push_liquidation(&mut self, time: f64, price: f64, usd: f64, long: bool, at: f64) {
        self.chart.push_liquidation(time, price, usd, long, at as i64);
    }

    /// The venue's mark, drawn beside each liquidation ping.
    pub fn set_logo(&mut self, box_size: f32, spec: &str) {
        self.text.set_mark(box_size, spec);
        self.chart.touch();
    }

    pub fn clear_liquidations(&mut self) {
        self.chart.clear_liquidations();
    }

    pub fn set_studies(&mut self, raw: &str) {
        self.chart.set_studies(raw);
    }

    pub fn set_hidden(&mut self, raw: &str) {
        self.chart.set_hidden(raw);
    }

    pub fn set_log_scale(&mut self, log: bool) {
        self.chart.set_log_scale(log);
    }

    /// Negative clears the override.
    pub fn set_price_decimals(&mut self, decimals: i32) {
        self.chart.set_price_decimals(usize::try_from(decimals).ok());
    }

    pub fn set_pane_weights(&mut self, raw: &str) {
        self.chart.set_pane_weights(raw);
    }

    pub fn pane_weights(&self) -> String {
        self.chart.pane_weights()
    }

    pub fn set_theme_value(&mut self, key: &str, value: &str) -> bool {
        let ok = self.chart.set_theme_value(key, value);
        if ok && key == "fontPx" {
            let width = self.text.char_width(self.chart.theme().font_px);
            self.chart.set_char_width(width);
        }
        ok
    }

    pub fn set_message(&mut self, message: &str) {
        self.chart.set_message((!message.is_empty()).then(|| message.to_string()));
    }

    /// `[{id, kind, points: [{time, price}], label}]`.
    pub fn set_drawings(&mut self, list: &Array) {
        let shapes = list
            .iter()
            .filter_map(|v| {
                let kind = DrawingKind::parse(&get(&v, "kind")?.as_string()?)?;
                let points = Array::from(&get(&v, "points")?)
                    .iter()
                    .filter_map(|p| Some(Anchor { time: get(&p, "time")?.as_f64()?, price: get(&p, "price")?.as_f64()? }))
                    .collect();
                Some(DrawingShape { id: get(&v, "id").and_then(|i| i.as_f64()).map(|i| i as i64), kind, points, label: get(&v, "label").and_then(|l| l.as_string()) })
            })
            .collect();
        self.chart.set_drawings(shapes);
    }

    /// NaN clears the selection.
    pub fn set_selected(&mut self, id: f64) {
        self.chart.set_selected((!id.is_nan()).then_some(id as i64));
    }

    /// An empty string selects the pointer.
    pub fn set_tool(&mut self, tool: &str) {
        self.chart.set_tool(DrawingKind::parse(tool));
    }

    pub fn reset_view(&mut self) {
        self.chart.reset_view();
    }

    pub fn auto_scale(&self) -> bool {
        self.chart.auto_scale()
    }

    pub fn set_auto_scale(&mut self) {
        self.chart.set_auto_scale();
    }

    /// NaN shows every bar.
    pub fn show_range(&mut self, days: f64) {
        self.chart.show_range((!days.is_nan()).then_some(days));
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) {
        self.chart.pointer_move(x, y);
    }

    pub fn pointer_down(&mut self, x: f32, y: f32, button: u16) {
        self.chart.pointer_down(x, y, button);
    }

    pub fn pointer_up(&mut self, x: f32, y: f32) {
        self.chart.pointer_up(x, y);
    }

    pub fn pointer_leave(&mut self) {
        self.chart.pointer_leave();
    }

    pub fn double_click(&mut self, x: f32, y: f32) {
        self.chart.double_click(x, y);
    }

    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.chart.wheel(x, y, dx, dy)
    }

    pub fn key(&mut self, key: &str) -> bool {
        self.chart.key(key)
    }

    pub fn cursor(&self) -> String {
        self.chart.cursor().css().to_string()
    }

    pub fn precision(&self) -> u32 {
        self.chart.precision() as u32
    }

    /// Draw if anything changed since the last frame. Returns whether it drew.
    pub fn draw(&mut self, now: f64) -> bool {
        if !self.chart.needs_render(now as i64) {
            return false;
        }
        let frame = self.chart.render(now as i64);
        self.gl.draw(frame);
        self.text.draw(frame);
        true
    }

    pub fn take_events(&mut self) -> Array {
        self.chart
            .take_events()
            .into_iter()
            .map(|e| {
                let o = obj();
                match e {
                    Event::DrawingCreated { kind, points } => {
                        set(&o, "type", "drawingCreated");
                        set(&o, "kind", kind.as_str());
                        let list: Array = points.iter().map(|p| JsValue::from(js::point(p.time, p.price))).collect();
                        set(&o, "points", list);
                    }
                    Event::SelectionChanged(id) => {
                        set(&o, "type", "selection");
                        set(&o, "id", opt(id.map(|i| i as f64)));
                    }
                    Event::DeleteRequested(id) => {
                        set(&o, "type", "delete");
                        set(&o, "id", id as f64);
                    }
                    Event::DrawingChanged { id, points } => {
                        set(&o, "type", "drawingChanged");
                        set(&o, "id", id as f64);
                        let list: Array = points.iter().map(|p| JsValue::from(js::point(p.time, p.price))).collect();
                        set(&o, "points", list);
                    }
                    Event::EditRequested(id) => {
                        set(&o, "type", "editRequested");
                        set(&o, "id", id as f64);
                    }
                    Event::ToolFinished => set(&o, "type", "toolFinished"),
                    Event::NeedHistory { before } => {
                        set(&o, "type", "needHistory");
                        set(&o, "before", before as f64);
                    }
                    Event::PaneWeightsChanged(value) => {
                        set(&o, "type", "paneWeights");
                        set(&o, "value", value);
                    }
                    Event::AutoScaleChanged(on) => {
                        set(&o, "type", "autoScale");
                        set(&o, "value", on);
                    }
                }
                JsValue::from(o)
            })
            .collect()
    }

    pub fn legend(&self) -> JsValue {
        let l = self.chart.legend();
        let o = obj();
        set(&o, "index", opt(l.index.map(|i| i as f64)));
        set(&o, "time", l.time as f64);
        set(&o, "ohlc", l.ohlc);
        set(&o, "change", l.change);
        set(&o, "up", l.up);
        set(&o, "volume", l.volume);
        let values = obj();
        for (k, v) in l.values {
            set(&values, &k, v);
        }
        set(&o, "values", values);
        o.into()
    }

    pub fn studies(&self) -> Array {
        self.chart
            .studies_meta()
            .into_iter()
            .map(|s| {
                let o = obj();
                set(&o, "id", s.id);
                set(&o, "label", s.label);
                set(&o, "group", s.group);
                set(&o, "hidden", s.hidden);
                let lines: Array = s
                    .lines
                    .into_iter()
                    .map(|(id, label, color)| {
                        let l = obj();
                        set(&l, "id", id);
                        set(&l, "label", label);
                        set(&l, "color", color.css());
                        JsValue::from(l)
                    })
                    .collect();
                set(&o, "lines", lines);
                let stats: Array = s
                    .stats
                    .into_iter()
                    .map(|(label, value)| {
                        let st = obj();
                        set(&st, "label", label);
                        set(&st, "value", value);
                        JsValue::from(st)
                    })
                    .collect();
                set(&o, "stats", stats);
                JsValue::from(o)
            })
            .collect()
    }
}

/// A `.tdc` payload (already inflated) as `{interval, time, open, high, low, close, volume}`.
#[wasm_bindgen]
pub fn decode_partition(bytes: &[u8]) -> Result<Object, JsValue> {
    let ticks = TickCandles::decode(bytes).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let o = columns(&ticks.to_candles());
    set(&o, "interval", ticks.interval.as_str());
    Ok(o)
}

/// Fold candles into a chart timeframe, dropping a bucket that has not closed.
#[wasm_bindgen]
pub fn fold_candles(tf: &str, time: &[f64], open: &[f64], high: &[f64], low: &[f64], close: &[f64], volume: &[f64], now: f64) -> Result<Object, JsValue> {
    Ok(columns(&fold(&candles(time, open, high, low, close, volume), timeframe(tf)?, now as i64)))
}

#[wasm_bindgen]
pub fn timeframes() -> Array {
    ChartTimeframe::ALL
        .iter()
        .map(|tf| {
            let o = obj();
            set(&o, "value", tf.as_str());
            set(&o, "label", tf.label());
            set(&o, "group", match tf.source().interval {
                Interval::M1 | Interval::M5 | Interval::M15 | Interval::M30 => 0,
                Interval::H1 | Interval::H2 | Interval::H4 | Interval::H6 | Interval::H12 => 1,
                Interval::D1 => 2,
                Interval::W1 => 3,
                Interval::Mo1 => 4,
            });
            JsValue::from(o)
        })
        .collect()
}

/// Where a timeframe's bars come from: `{interval, group, base, live, barMs, baseMs}`.
#[wasm_bindgen]
pub fn timeframe_info(tf: &str) -> Result<Object, JsValue> {
    let tf = timeframe(tf)?;
    let source = tf.source();
    let approx = |i: Interval| i.ms().unwrap_or(30 * tradoor_core::time::DAY) as f64;
    let o = obj();
    set(&o, "timeframe", tf.as_str());
    set(&o, "interval", source.interval.as_str());
    set(&o, "group", source.group);
    set(&o, "base", tf.history_base().as_str());
    set(&o, "live", tf.live_interval().map_or(JsValue::NULL, |i| JsValue::from_str(i.as_str())));
    set(&o, "barMs", approx(source.interval) * f64::from(source.group));
    set(&o, "baseMs", approx(tf.history_base()));
    set(&o, "intervalMs", approx(source.interval));
    Ok(o)
}

#[wasm_bindgen]
pub fn study_catalog() -> Array {
    STUDIES
        .iter()
        .map(|d| {
            let o = obj();
            set(&o, "kind", d.kind.as_str());
            set(&o, "name", d.name);
            set(&o, "short", d.short);
            set(&o, "group", d.group.as_str());
            set(&o, "about", d.about);
            let params: Array = d
                .params
                .iter()
                .map(|p| {
                    let po = obj();
                    set(&po, "name", p.name);
                    set(&po, "default", p.default);
                    set(&po, "min", p.min);
                    set(&po, "max", p.max);
                    JsValue::from(po)
                })
                .collect();
            set(&o, "params", params);
            JsValue::from(o)
        })
        .collect()
}

/// The forming bar for one market and interval.
#[wasm_bindgen]
pub struct LiveFeedEngine {
    feed: LiveFeed,
}

#[wasm_bindgen]
impl LiveFeedEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(tf: &str) -> Result<LiveFeedEngine, JsValue> {
        Ok(Self { feed: LiveFeed::new(timeframe(tf)?) })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_kline(&mut self, open_time: f64, open: f64, high: f64, low: f64, close: f64, volume: f64, last_trade_id: f64, closed: bool) -> bool {
        self.feed.apply_kline(Kline { open_time: open_time as i64, open, high, low, close, volume, last_trade_id: last_trade_id as i64, closed })
    }

    pub fn apply_trade(&mut self, time: f64, price: f64, qty: f64, last_trade_id: f64) -> bool {
        self.feed.apply_trade(time as i64, price, qty, last_trade_id as i64)
    }

    /// `{time, open, high, low, close, volume, closed}` or null.
    pub fn bar(&self) -> JsValue {
        match self.feed.current() {
            None => JsValue::NULL,
            Some(b) => {
                let o = obj();
                set(&o, "time", b.open_time as f64);
                set(&o, "open", b.open);
                set(&o, "high", b.high);
                set(&o, "low", b.low);
                set(&o, "close", b.close);
                set(&o, "volume", b.volume);
                set(&o, "closed", b.closed);
                o.into()
            }
        }
    }
}

/// The session a timeframe's profile describes and the interval it is built from.
#[wasm_bindgen]
pub fn profile_spec(tf: &str) -> Result<JsValue, JsValue> {
    Ok(match profile_spec_for(timeframe(tf)?) {
        None => JsValue::NULL,
        Some(spec) => {
            let o = obj();
            set(&o, "session", spec.session.as_str());
            set(&o, "from", spec.from.as_str());
            o.into()
        }
    })
}

/// TPO sessions from candles at the profile's source interval.
#[wasm_bindgen]
pub fn tpo(tf: &str, time: &[f64], open: &[f64], high: &[f64], low: &[f64], close: &[f64], volume: &[f64], max_sessions: u32) -> Result<JsValue, JsValue> {
    let Some(spec) = profile_spec_for(timeframe(tf)?) else { return Ok(JsValue::NULL) };
    let data = tradoor_core::chart::compute_tpo(&candles(time, open, high, low, close, volume), spec, max_sessions as usize);
    let o = obj();
    set(&o, "session", spec.session.as_str());
    set(&o, "from", spec.from.as_str());
    set(&o, "rowSize", data.row_size);
    set(&o, "gridLow", data.grid_low);
    set(&o, "reason", data.reason.map_or(JsValue::NULL, JsValue::from));
    let sessions: Array = data
        .sessions
        .iter()
        .map(|s| {
            let so = obj();
            set(&so, "start", s.session_start as f64);
            set(&so, "end", s.session_end as f64);
            set(&so, "periods", s.periods.len() as u32);
            set(&so, "high", s.high);
            set(&so, "low", s.low);
            set(&so, "poc", s.poc);
            set(&so, "vah", s.vah);
            set(&so, "val", s.val);
            let singles: Array = s.single_prints.iter().map(|i| JsValue::from_f64(*i as f64)).collect();
            set(&so, "singlePrints", singles);
            set(&so, "ib", s.initial_balance.map_or(JsValue::NULL, |(h, l)| Array::of2(&h.into(), &l.into()).into()));
            let rows: Array = s
                .rows
                .iter()
                .map(|r| {
                    let ro = obj();
                    set(&ro, "index", r.index as f64);
                    set(&ro, "periods", js_sys::Uint32Array::from(r.periods.iter().map(|p| *p as u32).collect::<Vec<_>>().as_slice()));
                    JsValue::from(ro)
                })
                .collect();
            set(&so, "rows", rows);
            JsValue::from(so)
        })
        .collect();
    set(&o, "sessions", sessions);
    Ok(o.into())
}

/// Indicator values on the last closed bar, and the prior session's profile
/// levels built from `src_*` candles at the profile's source interval.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn indicator_values(
    tf: &str,
    time: &[f64],
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    volume: &[f64],
    src_time: &[f64],
    src_open: &[f64],
    src_high: &[f64],
    src_low: &[f64],
    src_close: &[f64],
    src_volume: &[f64],
    basis: &str,
) -> Result<JsValue, JsValue> {
    let tf = timeframe(tf)?;
    let bars = candles(time, open, high, low, close, volume);
    let Some(last) = bars.last() else { return Ok(JsValue::NULL) };
    let engine = engine_series(&bars);
    let closes: Vec<Option<f64>> = bars.iter().map(|c| Some(c.close)).collect();
    let (r, s) = (rsi(&closes, 14), stoch_rsi(&closes, 14, 14, 3, 3));
    let at = |v: &[Option<f64>]| opt(v.last().copied().flatten());
    let o = obj();
    set(&o, "time", last.open_time as f64);
    set(&o, "close", last.close);
    set(&o, "ema12", at(&engine.ema12));
    set(&o, "ema25", at(&engine.ema25));
    set(&o, "ema200", at(&engine.ema200));
    set(&o, "rsi14", at(&r));
    set(&o, "stochK", at(&s.k));
    set(&o, "stochD", at(&s.d));
    let profile = match profile_spec_for(tf) {
        None => JsValue::NULL,
        Some(spec) => {
            let basis = if basis == "tpo" { ProfileBasis::Tpo } else { ProfileBasis::Volume };
            let source = candles(src_time, src_open, src_high, src_low, src_close, src_volume);
            let overlay = profile_overlay(&bars, &source, spec, basis);
            let p = obj();
            set(&p, "session", spec.session.as_str());
            set(&p, "from", spec.from.as_str());
            set(&p, "basis", basis.as_str());
            set(&p, "vah", at(&overlay.vah));
            set(&p, "val", at(&overlay.val));
            set(&p, "poc", at(&overlay.poc));
            set(&p, "covered", overlay.covered as u32);
            set(&p, "total", bars.len() as u32);
            p.into()
        }
    };
    set(&o, "profile", profile);
    Ok(o.into())
}
