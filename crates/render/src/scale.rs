//! Time and price scales.

/// The horizontal axis, in bars. Logical index `i` is bar `i`; positions between
/// and beyond bars are continuous, which is where drawings and projections live.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeScale {
    /// Pixels per bar.
    pub bar_spacing: f64,
    /// Bars of empty space between the last bar and the right edge (negative when
    /// scrolled back into history).
    pub right_offset: f64,
    /// Plot width in pixels.
    pub width: f64,
    pub bars: usize,
}

pub const MIN_BAR_SPACING: f64 = 0.5;
pub const MAX_BAR_SPACING: f64 = 80.0;

impl TimeScale {
    pub fn new() -> Self {
        Self { bar_spacing: 6.0, right_offset: 2.0, width: 0.0, bars: 0 }
    }

    fn right_logical(&self) -> f64 {
        self.bars as f64 - 1.0 + self.right_offset
    }

    /// The x of a bar's centre.
    pub fn x(&self, logical: f64) -> f64 {
        self.width - (self.right_logical() - logical + 0.5) * self.bar_spacing
    }

    pub fn logical(&self, x: f64) -> f64 {
        self.right_logical() + 0.5 - (self.width - x) / self.bar_spacing
    }

    /// Logical range covering the plot, left to right.
    pub fn visible(&self) -> (f64, f64) {
        (self.logical(0.0), self.logical(self.width))
    }

    pub fn nearest_bar(&self, x: f64) -> Option<usize> {
        (self.bars > 0).then(|| self.logical(x).round().clamp(0.0, (self.bars - 1) as f64) as usize)
    }

    /// Keep the bar under `x` under `x` while the spacing changes.
    pub fn zoom_at(&mut self, x: f64, factor: f64) {
        let anchor = self.logical(x);
        self.bar_spacing = (self.bar_spacing * factor).clamp(MIN_BAR_SPACING, MAX_BAR_SPACING);
        let right = anchor + (self.width - x) / self.bar_spacing - 0.5;
        self.right_offset = right - (self.bars as f64 - 1.0);
        self.clamp();
    }

    /// Move the bars `dx` pixels, the way dragging paper moves what is on it:
    /// dragging right brings older bars into view.
    pub fn pan(&mut self, dx: f64) {
        self.right_offset -= dx / self.bar_spacing;
        self.clamp();
    }

    /// At least a few bars always stay on screen, at either end.
    pub fn clamp(&mut self) {
        if self.width <= 0.0 || self.bars == 0 {
            return;
        }
        let visible = self.width / self.bar_spacing;
        let keep = 3.0f64.min(self.bars as f64);
        let max = visible - keep;
        let min = -(self.bars as f64) + keep;
        self.right_offset = self.right_offset.clamp(min.min(max), max.max(min));
    }

    /// The last `count` bars fill the plot, with `margin` bars of room at the right.
    pub fn show_last(&mut self, count: f64, margin: f64) {
        if self.width <= 0.0 {
            return;
        }
        self.bar_spacing = (self.width / (count.max(1.0) + margin)).clamp(MIN_BAR_SPACING, MAX_BAR_SPACING);
        self.right_offset = margin;
        self.clamp();
    }
}

impl Default for TimeScale {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    Normal,
    /// Equal PERCENTAGE moves take equal space.
    Log,
}

/// One pane's vertical axis. The range is kept in transformed space (ln for the
/// log scale), so zooming and margins mean the same thing under either mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceScale {
    pub mode: ScaleMode,
    pub top: f64,
    pub height: f64,
    /// Fractions of the height left empty above and below the data.
    pub margins: (f64, f64),
    /// Transformed (min, max).
    pub range: (f64, f64),
}

impl PriceScale {
    pub fn new(mode: ScaleMode, top: f64, height: f64, margins: (f64, f64)) -> Self {
        Self { mode, top, height, margins, range: (0.0, 1.0) }
    }

    pub fn transform(&self, price: f64) -> f64 {
        match self.mode {
            ScaleMode::Normal => price,
            ScaleMode::Log => price.max(1e-12).ln(),
        }
    }

    pub fn untransform(&self, t: f64) -> f64 {
        match self.mode {
            ScaleMode::Normal => t,
            ScaleMode::Log => t.exp(),
        }
    }

    /// Fit a price range, padding a flat one so it still has height.
    pub fn fit(&mut self, min: f64, max: f64) {
        if !(min.is_finite() && max.is_finite()) || min > max {
            return;
        }
        let (mut lo, mut hi) = (self.transform(min), self.transform(max));
        if hi - lo < 1e-12 {
            let pad = if lo == 0.0 { 1.0 } else { lo.abs() * 0.01 };
            lo -= pad;
            hi += pad;
        }
        self.range = (lo, hi);
    }

    fn inner(&self) -> (f64, f64) {
        let (mt, mb) = self.margins;
        (self.top + mt * self.height, (self.height * (1.0 - mt - mb)).max(1.0))
    }

    pub fn y(&self, price: f64) -> f64 {
        let (top, h) = self.inner();
        let (lo, hi) = self.range;
        top + (hi - self.transform(price)) / (hi - lo) * h
    }

    pub fn price(&self, y: f64) -> f64 {
        let (top, h) = self.inner();
        let (lo, hi) = self.range;
        self.untransform(hi - (y - top) / h * (hi - lo))
    }

    /// Move the range `dy` pixels, the way dragging paper moves what is on it:
    /// dragging down brings higher prices into view.
    pub fn pan(&mut self, dy: f64) {
        let (lo, hi) = self.range;
        let shift = dy / self.inner().1 * (hi - lo);
        if shift.is_finite() {
            self.range = (lo + shift, hi + shift);
        }
    }

    /// Stretch or squeeze the range around its centre.
    pub fn zoom(&mut self, factor: f64) {
        let (lo, hi) = self.range;
        let (mid, half) = ((lo + hi) / 2.0, (hi - lo) / 2.0 * factor);
        if half.is_finite() && half > 1e-12 {
            self.range = (mid - half, mid + half);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale() -> TimeScale {
        TimeScale { bar_spacing: 10.0, right_offset: 2.0, width: 1000.0, bars: 500 }
    }

    #[test]
    fn x_and_logical_are_inverse() {
        let t = scale();
        for l in [-3.5, 0.0, 250.25, 499.0, 510.0] {
            assert!((t.logical(t.x(l)) - l).abs() < 1e-9);
        }
        assert!((t.x(499.0) - (1000.0 - 25.0)).abs() < 1e-9, "the last bar sits 2.5 bars from the edge");
        assert_eq!(t.nearest_bar(t.x(123.2)), Some(123));
    }

    #[test]
    fn zoom_keeps_the_bar_under_the_pointer() {
        let mut t = scale();
        let before = t.logical(400.0);
        t.zoom_at(400.0, 1.7);
        assert!((t.logical(400.0) - before).abs() < 1e-9);
        t.zoom_at(400.0, 1e9);
        assert_eq!(t.bar_spacing, MAX_BAR_SPACING);
    }

    #[test]
    fn panning_is_clamped_so_bars_stay_visible() {
        let mut t = scale();
        t.pan(1e9);
        assert!(t.visible().0 <= (t.bars - 3) as f64, "bars remain on the left");
        t.pan(-1e12);
        assert!(t.visible().1 >= 2.0, "bars remain on the right");
    }

    #[test]
    fn price_scale_maps_both_modes_with_margins() {
        let mut p = PriceScale::new(ScaleMode::Normal, 0.0, 100.0, (0.2, 0.1));
        p.fit(100.0, 200.0);
        assert!((p.y(200.0) - 20.0).abs() < 1e-9 && (p.y(100.0) - 90.0).abs() < 1e-9);
        assert!((p.price(p.y(150.0)) - 150.0).abs() < 1e-9);

        let mut l = PriceScale::new(ScaleMode::Log, 0.0, 100.0, (0.0, 0.0));
        l.fit(10.0, 1000.0);
        assert!((l.y(100.0) - 50.0).abs() < 1e-9, "100 is halfway between 10 and 1000 on a log axis");
        assert!((l.price(50.0) - 100.0).abs() < 1e-9);

        let mut flat = PriceScale::new(ScaleMode::Normal, 0.0, 100.0, (0.0, 0.0));
        flat.fit(5.0, 5.0);
        assert!(flat.range.1 > flat.range.0);
    }
}
