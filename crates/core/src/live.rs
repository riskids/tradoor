//! The forming candle, folded from the exchange's streams.
//!
//! Display only: nothing here is stored. Binance's kline stream publishes about
//! once a second, which is a bar that lurches, so every trade is folded in
//! between. The kline still wins wherever the two disagree — it is the only source
//! of the bar's true open, of volume traded before the socket connected, and of
//! the boundary at which a bar is final. The one property defended below is that
//! the drawn bar NEVER GOES BACKWARDS.

use crate::ChartTimeframe;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveBar {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// Binance's taker buy volume for the forming bar, when it reports one.
    pub taker_buy_volume: Option<f64>,
    /// The exchange has marked the bar final. It is still not stored.
    pub closed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Kline {
    pub open_time: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub taker_buy_volume: Option<f64>,
    /// Last trade id included, or -1. Bybit has no comparable id and passes
    /// `i64::MAX`, so its kline always wins.
    pub last_trade_id: i64,
    pub closed: bool,
}

pub struct LiveFeed {
    timeframe: ChartTimeframe,
    bar: Option<LiveBar>,
    /// How far into the tape the bar accounts for — what tells a fresh kline from
    /// a second-old snapshot that would drag the price backwards.
    last_trade_id: i64,
}

impl LiveFeed {
    pub fn new(timeframe: ChartTimeframe) -> Self {
        Self { timeframe, bar: None, last_trade_id: -1 }
    }

    pub fn current(&self) -> Option<LiveBar> {
        self.bar
    }

    /// Returns true when the bar changed.
    pub fn apply_kline(&mut self, k: Kline) -> bool {
        if !k.close.is_finite() {
            return false;
        }
        let fresh = LiveBar { open_time: k.open_time, open: k.open, high: k.high, low: k.low, close: k.close, volume: k.volume, taker_buy_volume: k.taker_buy_volume, closed: k.closed };
        match self.bar {
            // The final kline for a bar the tape has already left.
            Some(bar) if k.open_time < bar.open_time => return false,
            Some(bar) if k.open_time == bar.open_time && k.last_trade_id < self.last_trade_id => {
                // A snapshot from before trades we already folded: it may widen the
                // extremes and correct the open, never rewind close or volume.
                self.bar = Some(LiveBar { high: fresh.high.max(bar.high), low: fresh.low.min(bar.low), close: bar.close, volume: fresh.volume.max(bar.volume), ..fresh });
            }
            _ => {
                self.bar = Some(fresh);
                self.last_trade_id = self.last_trade_id.max(k.last_trade_id);
            }
        }
        true
    }

    pub fn apply_trade(&mut self, time: i64, price: f64, qty: f64, last_trade_id: i64) -> bool {
        // Without a kline we do not know where the bar starts; an open invented
        // from the first tick seen would be fiction.
        let Some(bar) = self.bar else { return false };
        if !price.is_finite() {
            return false;
        }
        let end = self.timeframe.end(bar.open_time);
        if time >= end {
            // Only one bar is rolled: past that the pair was silent for a whole
            // bar, and the next kline reseeds instead of guessing.
            if time >= self.timeframe.end(end) {
                return false;
            }
            self.bar = Some(LiveBar { open_time: end, open: price, high: price, low: price, close: price, volume: qty, taker_buy_volume: None, closed: false });
        } else {
            if bar.closed {
                return false;
            }
            self.bar = Some(LiveBar { high: bar.high.max(price), low: bar.low.min(price), close: price, volume: bar.volume + if qty.is_finite() { qty } else { 0.0 }, ..bar });
        }
        self.last_trade_id = self.last_trade_id.max(last_trade_id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{utc, MINUTE};

    const M15: i64 = 15 * MINUTE;

    fn t() -> i64 {
        utc(2026, 7, 14) + 13 * 3_600_000 + 30 * MINUTE
    }

    fn kline(open_time: i64, o: f64, h: f64, l: f64, c: f64, v: f64, last: i64, closed: bool) -> Kline {
        Kline { open_time, open: o, high: h, low: l, close: c, volume: v, taker_buy_volume: None, last_trade_id: last, closed }
    }

    #[test]
    fn folds_trades_into_the_kline_bar() {
        let mut feed = LiveFeed::new(ChartTimeframe::M15);
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, false));
        feed.apply_trade(t() + 1_000, 102.0, 2.0, 1001);
        feed.apply_trade(t() + 2_000, 98.0, 3.0, 1002);
        assert_eq!(feed.current(), Some(LiveBar { open_time: t(), open: 100.0, high: 102.0, low: 98.0, close: 98.0, volume: 10.0, taker_buy_volume: None, closed: false }));
    }

    #[test]
    fn trades_before_the_first_kline_are_dropped() {
        let mut feed = LiveFeed::new(ChartTimeframe::M15);
        assert!(!feed.apply_trade(t() + 1_000, 100.0, 1.0, 900));
        assert_eq!(feed.current(), None);
    }

    #[test]
    fn an_older_snapshot_cannot_rewind() {
        let mut feed = LiveFeed::new(ChartTimeframe::M15);
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, false));
        feed.apply_trade(t() + 1_000, 105.0, 1.0, 1010);
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, false));
        let bar = feed.current().unwrap();
        assert_eq!((bar.close, bar.high, bar.volume), (105.0, 105.0, 6.0));

        feed.apply_kline(kline(t(), 100.0, 106.0, 99.0, 106.0, 7.0, 1010, false));
        let bar = feed.current().unwrap();
        assert_eq!((bar.volume, bar.close), (7.0, 106.0));
    }

    #[test]
    fn boundaries_roll_once_and_never_rewind() {
        let mut feed = LiveFeed::new(ChartTimeframe::M15);
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, false));
        feed.apply_trade(t() + M15 + 10, 107.0, 2.0, 1020);
        assert_eq!(feed.current(), Some(LiveBar { open_time: t() + M15, open: 107.0, high: 107.0, low: 107.0, close: 107.0, volume: 2.0, taker_buy_volume: None, closed: false }));
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, true));
        assert_eq!(feed.current().unwrap().open_time, t() + M15);

        let mut feed = LiveFeed::new(ChartTimeframe::M15);
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, false));
        feed.apply_trade(t() + 3 * M15, 200.0, 1.0, 1030);
        assert_eq!(feed.current().unwrap().close, 100.0);
    }

    #[test]
    fn a_final_bar_rejects_late_trades() {
        let mut feed = LiveFeed::new(ChartTimeframe::M15);
        feed.apply_kline(kline(t(), 100.0, 101.0, 99.0, 100.0, 5.0, 1000, true));
        feed.apply_trade(t() + 5_000, 500.0, 1.0, 999);
        assert_eq!(feed.current().unwrap().close, 100.0);
    }

    #[test]
    fn months_roll_on_the_calendar() {
        let mut feed = LiveFeed::new(ChartTimeframe::Mo1);
        let (aug, sep) = (utc(2026, 7, 1), utc(2026, 8, 1));
        feed.apply_kline(kline(aug, 100.0, 101.0, 99.0, 100.0, 5.0, 1000, false));
        feed.apply_trade(sep + 60_000, 111.0, 1.0, 1001);
        assert_eq!(feed.current().unwrap().open_time, sep);
    }
}
