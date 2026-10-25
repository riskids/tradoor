//! Every colour the chart draws with. The defaults are TradingView's own dark
//! theme — a black canvas, white up candles, blue down; a workspace can
//! override any of them by name.

use crate::color::Color;

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: Color,
    pub text: Color,
    pub border: Color,
    pub bull: Color,
    pub bear: Color,
    pub wick_bull: Color,
    pub wick_bear: Color,
    pub border_bull: Color,
    pub border_bear: Color,
    /// Price-pane horizontal grid at each axis tick.
    pub grid: Color,
    pub volume_bull: Color,
    pub volume_bear: Color,
    pub candle_alpha: u8,
    pub volume_alpha: u8,
    pub ema200: Color,
    pub ribbon_fast: Color,
    pub ribbon_slow: Color,
    pub ribbon_fill_alpha: u8,
    pub crosshair: Color,
    pub label_background: Color,
    /// Text on a `label_background` tag — the crosshair's price and time —
    /// a fixed colour rather than computed, so a light `label_background`
    /// reads as one flat tag instead of flipping its own text dark.
    pub label_text: Color,
    pub countdown_background: Color,
    pub countdown_text: Color,
    pub study_palette: [Color; 6],
    /// Drawings are the reader's opinion and get a hue no computed series uses.
    pub drawing: Color,
    pub drawing_selected: Color,
    /// Price alert lines and their axis tags.
    pub alert: Color,
    pub drawing_fill: Color,
    pub handle: Color,
    pub risk: Color,
    pub risk_fill: Color,
    pub reward: Color,
    pub reward_fill: Color,
    /// TPO blocks: early brackets faint, late ones strong.
    pub tpo: Color,
    pub tpo_poc: Color,
    pub font_px: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            // TradingView's default: a black canvas, white up candles, blue down.
            background: Color::hex(0x0f0f0f),
            text: Color::hex(0xb2b5be),
            border: Color::hex(0x2a2e39),
            bull: Color::hex(0xffffff),
            bear: Color::hex(0x2196f3),
            wick_bull: Color::hex(0xffffff),
            wick_bear: Color::hex(0x2196f3),
            border_bull: Color::hex(0xffffff),
            border_bear: Color::hex(0x2196f3),
            grid: Color::hex(0x1c1c1c),
            volume_bull: Color::hex(0xffffff),
            volume_bear: Color::hex(0x2196f3),
            candle_alpha: 0xcc,
            volume_alpha: 0x60,
            ema200: Color::hex(0xffb03a),
            ribbon_fast: Color::hex(0xaafd35),
            ribbon_slow: Color::hex(0xff293a),
            ribbon_fill_alpha: 51,
            crosshair: Color::hex(0x9598a1),
            label_background: Color::hex(0xffffff),
            label_text: Color::hex(0x131722),
            countdown_background: Color::hex(0x2a2f2a),
            countdown_text: Color::hex(0xd7ded7),
            study_palette: [Color::hex(0x7fb2ff), Color::hex(0xc58fff), Color::hex(0xffd166), Color::hex(0x4ecdc4), Color::hex(0xff8fab), Color::hex(0x9ae66e)],
            drawing: Color::hex(0xff5fd2),
            drawing_selected: Color::hex(0xffa8e6),
            alert: Color::hex(0x22ab94),
            drawing_fill: Color::hex(0xff5fd2).with_alpha(26),
            handle: Color::hex(0xffffff),
            risk: Color::hex(0xd97161),
            risk_fill: Color::hex(0xd97161).with_alpha(36),
            reward: Color::hex(0xa4c588),
            reward_fill: Color::hex(0xa4c588).with_alpha(36),
            tpo: Color::hex(0xd9dcd9),
            tpo_poc: Color::hex(0xf07a2a),
            font_px: 11.0,
        }
    }
}

impl Theme {
    /// Set one entry by the name a workspace stores it under. Returns false for an
    /// unknown name or an unparseable value, leaving the theme unchanged.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        if let Some(i) = key.strip_prefix("study").and_then(|n| n.parse::<usize>().ok()) {
            return match (self.study_palette.get_mut(i), Color::parse(value)) {
                (Some(slot), Some(c)) => {
                    *slot = c;
                    true
                }
                _ => false,
            };
        }
        if key == "fontPx" {
            return value.parse::<f32>().ok().filter(|v| (8.0..=20.0).contains(v)).map(|v| self.font_px = v).is_some();
        }
        if key == "candleAlpha" {
            return value.parse::<u8>().ok().map(|v| self.candle_alpha = v).is_some();
        }
        if key == "volumeAlpha" {
            return value.parse::<u8>().ok().map(|v| self.volume_alpha = v).is_some();
        }
        let Some(color) = Color::parse(value) else { return false };
        let slot = match key {
            "background" => &mut self.background,
            "text" => &mut self.text,
            "border" => &mut self.border,
            "bull" => &mut self.bull,
            "bear" => &mut self.bear,
            "wickBull" => &mut self.wick_bull,
            "wickBear" => &mut self.wick_bear,
            "borderBull" => &mut self.border_bull,
            "borderBear" => &mut self.border_bear,
            "grid" => &mut self.grid,
            "volumeBull" => &mut self.volume_bull,
            "volumeBear" => &mut self.volume_bear,
            "ema200" => &mut self.ema200,
            "ribbonFast" => &mut self.ribbon_fast,
            "ribbonSlow" => &mut self.ribbon_slow,
            "crosshair" => &mut self.crosshair,
            "labelBackground" => &mut self.label_background,
            "labelText" => &mut self.label_text,
            "drawing" => &mut self.drawing,
            "drawingSelected" => &mut self.drawing_selected,
            "alert" => &mut self.alert,
            "tpo" => &mut self.tpo,
            "tpoPoc" => &mut self.tpo_poc,
            _ => return false,
        };
        *slot = color;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_wick_bull_updates_the_field() {
        let mut t = Theme::default();
        assert!(t.set("wickBull", "#123456"));
        assert_eq!(t.wick_bull, Color::hex(0x123456));
        assert!(!t.set("wickBull", "not-a-color"));
        assert!(!t.set("nonsense", "#000000"));
    }
}
