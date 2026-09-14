//! The text layer: a 2D canvas over the GPU one.

use tradoor_render::{Align, Baseline, Frame};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{OffscreenCanvas, OffscreenCanvasRenderingContext2d as Ctx};

pub struct TextLayer {
    canvas: OffscreenCanvas,
    ctx: Ctx,
}

fn font(size: f32) -> String {
    format!("{size}px ui-monospace, SFMono-Regular, Menlo, Consolas, \"Liberation Mono\", monospace")
}

impl TextLayer {
    pub fn new(canvas: OffscreenCanvas) -> Result<Self, JsValue> {
        let ctx: Ctx = canvas.get_context("2d")?.ok_or("2D canvas is not available")?.dyn_into()?;
        Ok(Self { canvas, ctx })
    }

    /// Axis labels are monospace, so one glyph's width sizes every label.
    pub fn char_width(&self, size: f32) -> f32 {
        self.ctx.set_font(&font(size));
        self.ctx.measure_text("0").map_or(size * 0.6, |m| m.width() as f32)
    }

    pub fn draw(&mut self, frame: &Frame) {
        let (w, h) = ((frame.width * frame.dpr).round().max(1.0) as u32, (frame.height * frame.dpr).round().max(1.0) as u32);
        if self.canvas.width() != w {
            self.canvas.set_width(w);
        }
        if self.canvas.height() != h {
            self.canvas.set_height(h);
        }
        let ctx = &self.ctx;
        let dpr = f64::from(frame.dpr);
        let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
        ctx.clear_rect(0.0, 0.0, f64::from(frame.width), f64::from(frame.height));

        let mut current = (0.0f32, None::<Align>, None::<Baseline>);
        for t in &frame.texts {
            if let Some(c) = t.clip {
                ctx.save();
                ctx.begin_path();
                ctx.rect(f64::from(c.x), f64::from(c.y), f64::from(c.w), f64::from(c.h));
                ctx.clip();
            }
            if t.clip.is_some() || current.0 != t.size {
                ctx.set_font(&font(t.size));
            }
            if t.clip.is_some() || current.1 != Some(t.align) {
                ctx.set_text_align(match t.align {
                    Align::Left => "left",
                    Align::Center => "center",
                    Align::Right => "right",
                });
            }
            if t.clip.is_some() || current.2 != Some(t.baseline) {
                ctx.set_text_baseline(match t.baseline {
                    Baseline::Top => "top",
                    Baseline::Middle => "middle",
                    Baseline::Bottom => "bottom",
                });
            }
            current = (t.size, Some(t.align), Some(t.baseline));
            let (x, y) = (f64::from(t.x), f64::from(t.y));
            if let Some(bg) = t.background {
                let width = ctx.measure_text(&t.text).map_or(0.0, |m| m.width());
                let left = match t.align {
                    Align::Left => x,
                    Align::Center => x - width / 2.0,
                    Align::Right => x - width,
                };
                let height = f64::from(t.size) + 4.0;
                let top = match t.baseline {
                    Baseline::Top => y - 2.0,
                    Baseline::Middle => y - height / 2.0,
                    Baseline::Bottom => y - height + 2.0,
                };
                ctx.set_fill_style_str(&bg.css());
                ctx.fill_rect(left - 3.0, top, width + 6.0, height);
            }
            ctx.set_fill_style_str(&t.color.css());
            let _ = ctx.fill_text(&t.text, x, y);
            if t.clip.is_some() {
                ctx.restore();
                current = (0.0, None, None);
            }
        }
    }
}
