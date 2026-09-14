//! The list of markets a symbol search offers.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Product {
    /// `bybit-perp:BTCUSDT`.
    pub market: String,
    pub venue: &'static str,
    pub symbol: String,
    pub base: String,
    pub quote: String,
    pub kind: &'static str,
    /// Decimals of the exchange's tick size, when it publishes one.
    pub price_decimals: Option<u8>,
    pub quote_volume_24h: Option<f64>,
    pub last_price: Option<f64>,
    pub change_pct: Option<f64>,
}

pub(crate) fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    v?.as_str()?.parse::<f64>().ok().filter(|x| x.is_finite())
}

pub(crate) fn tick_decimals(tick: Option<&str>) -> Option<u8> {
    tradoor_codec::decimal::parse_decimal(tick?).map(|(_, scale)| scale)
}
