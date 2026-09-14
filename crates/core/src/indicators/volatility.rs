//! True Range, ATR, Bollinger Bands.

use super::series::{rma, sma, stdev};
use super::Series;
use crate::Candle;

/// `ta.tr(handle_na = true)`: the first bar has no previous close and uses
/// high − low, which is why ATR starts at index len−1.
pub fn true_range(candles: &[Candle]) -> Series {
    candles
        .iter()
        .enumerate()
        .map(|(i, c)| {
            Some(if i == 0 {
                c.high - c.low
            } else {
                let prev = candles[i - 1].close;
                (c.high - c.low).max((c.high - prev).abs()).max((c.low - prev).abs())
            })
        })
        .collect()
}

pub fn atr(candles: &[Candle], len: usize) -> Series {
    rma(&true_range(candles), len)
}

pub struct Bollinger {
    pub basis: Series,
    pub upper: Series,
    pub lower: Series,
}

pub fn bollinger(src: &[Option<f64>], len: usize, mult: f64) -> Bollinger {
    let basis = sma(src, len);
    let dev = stdev(src, len);
    let band = |sign: f64| -> Series {
        basis.iter().zip(&dev).map(|(b, d)| Some((*b)? + sign * mult * (*d)?)).collect()
    };
    Bollinger { upper: band(1.0), lower: band(-1.0), basis }
}
