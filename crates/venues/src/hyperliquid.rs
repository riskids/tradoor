//! Hyperliquid perpetuals, priced in USD and settled in USDC.
//!
//! Coins are named alone — `BTC`, or `kPEPE` for a thousand PEPE — and are
//! `hyperliquid-perp:BTCUSD` and `hyperliquid-perp:1000PEPEUSD` here.

use serde_json::Value;

use crate::catalog::{change, num, product, unique, Product};
use crate::{shape, Venue, VenueError};

pub const INFO: &str = "https://api.hyperliquid.xyz/info";
/// The body `INFO` is POSTed for the perpetuals and their 24h figures.
pub const META_AND_CTXS: &str = r#"{"type":"metaAndAssetCtxs"}"#;

/// `kPEPE` → `1000PEPE`, the multiplier other exchanges put in front.
pub fn base_of(name: &str) -> String {
    match name.strip_prefix('k') {
        Some(rest) if rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => format!("1000{rest}"),
        _ => name.to_string(),
    }
}

/// Listed perpetuals. Prices carry up to six decimals less the size decimals.
pub fn parse_products(body: &str) -> Result<Vec<Product>, VenueError> {
    let v: Value = serde_json::from_str(body)?;
    let universe = v[0]["universe"].as_array().ok_or_else(|| shape("hyperliquid universe"))?;
    let ctxs = v[1].as_array().ok_or_else(|| shape("hyperliquid contexts"))?;
    let list = universe
        .iter()
        .zip(ctxs)
        .filter(|(u, _)| u["isDelisted"] != true)
        .filter_map(|(u, c)| {
            let name = u["name"].as_str()?;
            let decimals = u["szDecimals"].as_u64().map(|s| 6u8.saturating_sub(s as u8));
            let last = num(c.get("markPx"));
            product(Venue::HyperliquidPerp, &base_of(name), "USD", name, decimals, num(c.get("dayNtlVlm")), last, change(num(c.get("prevDayPx")), last))
        })
        .collect();
    Ok(unique(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured 2026-09-23, trimmed.
    #[test]
    fn coins_become_usd_markets() {
        let body = r#"[{"universe":[{"szDecimals":5,"name":"BTC","maxLeverage":40},{"szDecimals":0,"name":"kPEPE","maxLeverage":10},{"szDecimals":1,"name":"MATIC","isDelisted":true}]},
            [{"markPx":"86882.0","prevDayPx":"85262.0","dayNtlVlm":"2758297422.27"},{"markPx":"0.009","prevDayPx":"0.009","dayNtlVlm":"1"},{"markPx":"0.3"}]]"#;
        let p = parse_products(body).unwrap();
        assert_eq!(p.iter().map(|p| p.market.as_str()).collect::<Vec<_>>(), ["hyperliquid-perp:BTCUSD", "hyperliquid-perp:1000PEPEUSD"]);
        assert_eq!((p[0].native.as_deref(), p[0].price_decimals), (Some("BTC"), Some(1)));
        assert_eq!(p[1].native.as_deref(), Some("kPEPE"));
        assert_eq!(base_of("kSHIB"), "1000SHIB");
        assert_eq!(base_of("kas"), "kas");
    }
}
