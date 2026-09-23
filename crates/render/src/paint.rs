//! A frame: coloured triangles for the GPU layer, text for the 2D layer.

use crate::color::Color;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// A run of triangles sharing one clip rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Batch {
    pub first: u32,
    pub count: u32,
    pub clip: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Baseline {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextCmd {
    pub x: f32,
    pub y: f32,
    pub text: String,
    pub color: Color,
    pub align: Align,
    pub baseline: Baseline,
    pub size: f32,
    /// A filled box behind the text, the way an axis badge is drawn.
    pub background: Option<Color>,
    pub clip: Option<Rect>,
}

/// A badge the platform draws for us — the venue's own mark, whose artwork the
/// engine never holds. Only where, how big, and how strongly it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconCmd {
    /// Centre of the square the mark is drawn in.
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub alpha: u8,
    /// Which mark: the platform numbers them, the engine only passes it on.
    pub mark: u8,
    pub clip: Option<Rect>,
}

#[derive(Debug, Default, Clone)]
pub struct Frame {
    pub width: f32,
    pub height: f32,
    pub dpr: f32,
    pub background: Color,
    /// x, y per vertex, CSS pixels.
    pub positions: Vec<f32>,
    /// r, g, b, a per vertex.
    pub colors: Vec<u8>,
    pub batches: Vec<Batch>,
    pub texts: Vec<TextCmd>,
    pub icons: Vec<IconCmd>,
    /// Soften slanted drawing lines with a one-pixel fade at their edges. For a
    /// GPU surface, where multisampling alone leaves a thin slanted line
    /// stepped and beaded; a 2D canvas smooths its own edges, and fills a
    /// triangle in one colour, so it is not asked.
    pub smooth: bool,
}

impl Frame {
    /// Start a frame, keeping the buffers' capacity from the last one.
    pub fn begin(&mut self, width: f32, height: f32, dpr: f32, background: Color) {
        self.width = width;
        self.height = height;
        self.dpr = dpr.max(0.5);
        self.background = background;
        self.positions.clear();
        self.colors.clear();
        self.batches.clear();
        self.texts.clear();
        self.icons.clear();
        self.batches.push(Batch { first: 0, count: 0, clip: None });
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len() / 2
    }

    pub fn clip(&mut self, clip: Option<Rect>) {
        let first = self.vertex_count() as u32;
        let last = self.batches.last_mut().expect("begin() pushes a batch");
        if last.clip == clip {
            return;
        }
        if last.count == 0 {
            last.clip = clip;
        } else {
            self.batches.push(Batch { first, count: 0, clip });
        }
    }

    fn vertex(&mut self, x: f32, y: f32, c: Color) {
        self.positions.extend_from_slice(&[x, y]);
        self.colors.extend_from_slice(&[c.r, c.g, c.b, c.a]);
        self.batches.last_mut().expect("begin() pushes a batch").count += 1;
    }

    pub fn triangle(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), color: Color) {
        if color.a == 0 {
            return;
        }
        self.vertex(a.0, a.1, color);
        self.vertex(b.0, b.1, color);
        self.vertex(c.0, c.1, color);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        self.triangle((x, y), (x + w, y), (x, y + h), color);
        self.triangle((x + w, y), (x + w, y + h), (x, y + h), color);
    }

    pub fn snap(&self, v: f32) -> f32 {
        (v * self.dpr).round() / self.dpr
    }

    /// Edges snapped to device pixels, so a one-pixel line is one device row
    /// rather than two half-covered ones.
    pub fn crisp_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let min = 1.0 / self.dpr;
        let (x0, y0) = (self.snap(x), self.snap(y));
        let (x1, y1) = (self.snap(x + w), self.snap(y + h));
        self.rect(x0, y0, (x1 - x0).max(min), (y1 - y0).max(min), color);
    }

    pub fn hline(&mut self, x0: f32, x1: f32, y: f32, width: f32, color: Color) {
        let w = width.max(1.0 / self.dpr);
        self.crisp_rect(x0.min(x1), y - w / 2.0, (x1 - x0).abs(), w, color);
    }

    pub fn vline(&mut self, x: f32, y0: f32, y1: f32, width: f32, color: Color) {
        let w = width.max(1.0 / self.dpr);
        self.crisp_rect(x - w / 2.0, y0.min(y1), w, (y1 - y0).abs(), color);
    }

    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = dx.hypot(dy);
        if len == 0.0 || !len.is_finite() {
            return;
        }
        let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
        self.triangle((x0 + nx, y0 + ny), (x1 + nx, y1 + ny), (x0 - nx, y0 - ny), color);
        self.triangle((x1 + nx, y1 + ny), (x1 - nx, y1 - ny), (x0 - nx, y0 - ny), color);
    }

    /// A line with soft edges where `smooth` is on and it is slanted; see `smooth`.
    pub fn smooth_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = dx.hypot(dy);
        if !self.smooth || dx == 0.0 || dy == 0.0 || len == 0.0 || !len.is_finite() {
            self.line(x0, y0, x1, y1, width, color);
            return;
        }
        // One device pixel of fade each side, taken from the solid core so the
        // line covers as much as it did: core + two half-covered fades = width.
        let fade = 1.0 / self.dpr;
        let solid = if width >= fade { color } else { color.with_alpha((f32::from(color.a) * width / fade) as u8) };
        let core = ((width - fade) / 2.0).max(0.0);
        let (ux, uy) = (-dy / len, dx / len);
        if core > 0.0 {
            self.line(x0, y0, x1, y1, core * 2.0, solid);
        }
        let clear = solid.with_alpha(0);
        for side in [1.0f32, -1.0] {
            let (ix, iy) = (ux * core * side, uy * core * side);
            let (ox, oy) = (ux * (core + fade) * side, uy * (core + fade) * side);
            let (a, b) = ((x0 + ix, y0 + iy), (x1 + ix, y1 + iy));
            let (c, d) = ((x0 + ox, y0 + oy), (x1 + ox, y1 + oy));
            self.vertex(a.0, a.1, solid);
            self.vertex(b.0, b.1, solid);
            self.vertex(c.0, c.1, clear);
            self.vertex(b.0, b.1, solid);
            self.vertex(d.0, d.1, clear);
            self.vertex(c.0, c.1, clear);
        }
    }

    /// `dashed_line` with `smooth_line`'s edges.
    pub fn smooth_dashed_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color, dash: f32, gap: f32) {
        let len = (x1 - x0).hypot(y1 - y0);
        if !self.smooth || x0 == x1 || y0 == y1 || len == 0.0 || !len.is_finite() {
            self.dashed_line(x0, y0, x1, y1, width, color, dash, gap);
            return;
        }
        let (ux, uy) = ((x1 - x0) / len, (y1 - y0) / len);
        let mut d = 0.0;
        while d < len {
            let e = (d + dash).min(len);
            self.smooth_line(x0 + ux * d, y0 + uy * d, x0 + ux * e, y0 + uy * e, width, color);
            d += dash + gap;
        }
    }

    pub fn dashed_line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color, dash: f32, gap: f32) {
        let len = (x1 - x0).hypot(y1 - y0);
        if len == 0.0 || !len.is_finite() {
            return;
        }
        let (ux, uy) = ((x1 - x0) / len, (y1 - y0) / len);
        let horizontal = y0 == y1;
        let mut d = 0.0;
        while d < len {
            let e = (d + dash).min(len);
            if horizontal {
                self.hline(x0 + ux * d, x0 + ux * e, y0, width, color);
            } else if x0 == x1 {
                self.vline(x0, y0 + uy * d, y0 + uy * e, width, color);
            } else {
                self.line(x0 + ux * d, y0 + uy * d, x0 + ux * e, y0 + uy * e, width, color);
            }
            d += dash + gap;
        }
    }

    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: Color) {
        for w in points.windows(2) {
            self.line(w[0].0, w[0].1, w[1].0, w[1].1, width, color);
        }
        // Squares at the joints fill the notch a thick line leaves at each turn.
        if width > 1.5 && points.len() > 2 {
            for p in &points[1..points.len() - 1] {
                self.rect(p.0 - width / 2.0, p.1 - width / 2.0, width, width, color);
            }
        }
    }

    /// A band between two lines sampled at the same x positions.
    pub fn band(&mut self, upper: &[(f32, f32)], lower: &[(f32, f32)], color: Color) {
        for i in 1..upper.len().min(lower.len()) {
            self.triangle(upper[i - 1], lower[i - 1], upper[i], color);
            self.triangle(upper[i], lower[i - 1], lower[i], color);
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, color: Color) {
        let segments = ((r * 2.5) as usize).clamp(8, 32);
        let step = std::f32::consts::TAU / segments as f32;
        for i in 0..segments {
            let (a0, a1) = (i as f32 * step, (i + 1) as f32 * step);
            self.triangle((cx, cy), (cx + r * a0.cos(), cy + r * a0.sin()), (cx + r * a1.cos(), cy + r * a1.sin()), color);
        }
    }

    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, color: Color) {
        let (l, r, t, b) = (x.min(x + w), x.max(x + w), y.min(y + h), y.max(y + h));
        self.hline(l, r, t, width, color);
        self.hline(l, r, b, width, color);
        self.vline(l, t, b, width, color);
        self.vline(r, t, b, width, color);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, x: f32, y: f32, text: impl Into<String>, color: Color, align: Align, baseline: Baseline, size: f32, background: Option<Color>, clip: Option<Rect>) {
        self.texts.push(TextCmd { x, y, text: text.into(), color, align, baseline, size, background, clip });
    }

    pub fn icon(&mut self, x: f32, y: f32, size: f32, alpha: u8, mark: u8, clip: Option<Rect>) {
        if alpha == 0 || size <= 0.0 {
            return;
        }
        self.icons.push(IconCmd { x, y, size, alpha, mark, clip });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_smooth_slanted_line_fades_out_at_its_edges() {
        let mut f = Frame::default();
        f.begin(100.0, 100.0, 1.0, Color::hex(0));
        let white = Color::hex(0xffffff);
        f.smooth_line(10.0, 10.0, 90.0, 60.0, 2.0, white);
        assert_eq!(f.vertex_count(), 6, "plain until asked");

        f.begin(100.0, 100.0, 1.0, Color::hex(0));
        f.smooth = true;
        f.smooth_line(10.0, 10.0, 90.0, 60.0, 2.0, white);
        assert_eq!(f.vertex_count(), 18, "a core and a fade each side");
        let clear = f.colors.chunks(4).filter(|c| c[3] == 0).count();
        assert_eq!(clear, 6, "the fades' outer edges are transparent");
        f.begin(100.0, 100.0, 1.0, Color::hex(0));
        f.smooth_line(10.0, 50.0, 90.0, 50.0, 2.0, white);
        assert_eq!(f.vertex_count(), 6, "a level line is crisp already");
    }

    #[test]
    fn batches_split_on_clip_changes_only() {
        let mut f = Frame::default();
        f.begin(100.0, 100.0, 2.0, Color::hex(0));
        f.rect(0.0, 0.0, 10.0, 10.0, Color::hex(0xffffff));
        f.clip(Some(Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 }));
        f.clip(Some(Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 }));
        f.rect(0.0, 0.0, 10.0, 10.0, Color::hex(0xffffff));
        f.rect(0.0, 0.0, 10.0, 10.0, Color::TRANSPARENT);
        assert_eq!(f.batches.len(), 2);
        assert_eq!((f.batches[0].count, f.batches[1].first, f.batches[1].count), (6, 6, 6));
        assert_eq!(f.colors.len(), f.positions.len() * 2);
    }

    #[test]
    fn crisp_rects_land_on_device_pixels() {
        let mut f = Frame::default();
        f.begin(100.0, 100.0, 2.0, Color::hex(0));
        f.crisp_rect(10.3, 5.1, 0.1, 3.0, Color::hex(0xffffff));
        for v in &f.positions {
            assert_eq!((v * 2.0).fract(), 0.0, "{v} is not on a device pixel");
        }
    }
}
