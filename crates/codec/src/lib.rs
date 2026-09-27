//! tradoor-codec — how candle history is stored.
//!
//! One file per market, per base interval (1m, 1h, 1d), per closed calendar
//! period. Prices and volumes are kept as INTEGER ticks parsed straight from the
//! exchange's decimal strings, so nothing is lost to float rounding on the way
//! to disk, and aggregating minutes into hours is exact. The columns are
//! delta-encoded varints; the file is gzipped so the browser can inflate it with
//! the built-in `DecompressionStream`. See docs/format.md for the byte layout.
#![forbid(unsafe_code)]

pub mod decimal;
#[cfg(feature = "manifest")]
pub mod manifest;
pub mod partition;
pub mod period;
pub mod varint;

pub use partition::{summarize, CodecError, RawCandle, Summary, TickCandles};
pub use period::{max_tier, Period, Tier};
