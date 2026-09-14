//! A kline as an exchange sent it, before parsing into ticks.

use tradoor_codec::{CodecError, RawCandle, TickCandles};
use tradoor_core::Interval;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedKline {
    pub open_time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub quote_volume: Option<String>,
    pub taker_buy_volume: Option<String>,
    pub trades: Option<u64>,
}

impl OwnedKline {
    pub fn as_raw(&self) -> RawCandle<'_> {
        RawCandle {
            open_time: self.open_time,
            open: &self.open,
            high: &self.high,
            low: &self.low,
            close: &self.close,
            volume: &self.volume,
            quote_volume: self.quote_volume.as_deref(),
            taker_buy_volume: self.taker_buy_volume.as_deref(),
            trades: self.trades,
        }
    }
}

pub fn to_ticks(interval: Interval, klines: &[OwnedKline]) -> Result<TickCandles, CodecError> {
    let raw: Vec<RawCandle<'_>> = klines.iter().map(OwnedKline::as_raw).collect();
    TickCandles::from_raw(interval, &raw)
}
