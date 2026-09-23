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
    /// The exchange's own name for the market, where it is not `symbol`:
    /// `BTC-USDT-SWAP`, `BTC/USD`, Hyperliquid's `kPEPE`, Lighter's market number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native: Option<String>,
    /// Decimals of the exchange's tick size, when it publishes one.
    pub price_decimals: Option<u8>,
    pub quote_volume_24h: Option<f64>,
    pub last_price: Option<f64>,
    pub change_pct: Option<f64>,
}

/// A number sent as a string, as most exchanges do, or as a number.
pub(crate) fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    let v = v?;
    v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()).filter(|x| x.is_finite())
}

pub(crate) fn tick_decimals(tick: Option<&str>) -> Option<u8> {
    tradoor_codec::decimal::parse_decimal(tick?).map(|(_, scale)| scale)
}

/// This app's spelling of a market: letters and digits only, upper case.
pub(crate) fn canonical(base: &str, quote: &str) -> String {
    format!("{base}{quote}").chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase()
}

/// Decimals of a tick given as a number, such as Kraken Futures' `0.5`.
pub(crate) fn decimals_of(tick: f64) -> Option<u8> {
    (tick > 0.0 && tick.is_finite()).then(|| (0..=12u8).find(|d| (tick * 10f64.powi(i32::from(*d))).fract().abs() < 1e-9).unwrap_or(12))
}

/// A percent change from an open and a last price.
pub(crate) fn change(open: Option<f64>, last: Option<f64>) -> Option<f64> {
    match (open, last) {
        (Some(o), Some(l)) if o > 0.0 => Some((l - o) / o * 100.0),
        _ => None,
    }
}

/// A product in this app's spelling, or none when the name cannot be one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn product(venue: crate::Venue, base: &str, quote: &str, native: &str, price_decimals: Option<u8>, quote_volume_24h: Option<f64>, last_price: Option<f64>, change_pct: Option<f64>) -> Option<Product> {
    let symbol = canonical(base, quote);
    let market = crate::MarketRef::new(venue, &symbol)?;
    Some(Product {
        market: market.to_string(),
        venue: venue.id(),
        native: (native != symbol).then(|| native.to_string()),
        symbol,
        base: base.to_ascii_uppercase(),
        quote: quote.to_ascii_uppercase(),
        kind: venue.kind().as_str(),
        price_decimals,
        quote_volume_24h,
        last_price,
        change_pct,
    })
}

/// Keep the first of each symbol: two exchange names that spell alike here
/// would be one market to the chart.
pub(crate) fn unique(mut products: Vec<Product>) -> Vec<Product> {
    let mut seen = std::collections::HashSet::new();
    products.retain(|p| seen.insert(p.market.clone()));
    products
}
