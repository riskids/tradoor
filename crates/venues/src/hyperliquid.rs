//! Hyperliquid perpetuals, priced in USD and settled in USDC.
//!
//! Coins are named alone — `BTC`, or `kPEPE` for a thousand PEPE — and are
//! `hyperliquid-perp:BTCUSD` and `hyperliquid-perp:1000PEPEUSD` here. Beside
//! them, the perpetuals trade.xyz deploys on Hyperliquid (HIP-3) — US stocks
//! and indices, commodities, currencies — named with their deployer's prefix,
//! `xyz:SP500`, and `hyperliquid-perp:SP500USD` here: the same API, candles
//! and streams, asked for by that name.

use serde_json::Value;

use crate::catalog::{change, num, product, unique, Product};
use crate::{shape, Venue, VenueError};

pub const INFO: &str = "https://api.hyperliquid.xyz/info";
/// The body `INFO` is POSTed for the perpetuals and their 24h figures.
pub const META_AND_CTXS: &str = r#"{"type":"metaAndAssetCtxs"}"#;
/// The same for trade.xyz's perpetuals.
pub const XYZ_META_AND_CTXS: &str = r#"{"type":"metaAndAssetCtxs","dex":"xyz"}"#;

/// What trade.xyz's index perpetuals follow, for the search to say.
fn xyz_name(base: &str) -> String {
    let what = match base {
        "SP500" => "S&P 500",
        "XYZ100" => "Nasdaq-100",
        "JP225" => "Nikkei 225",
        "KR200" => "KOSPI 200",
        "DXY" => "US Dollar Index",
        other => other,
    };
    format!("{what} · trade.xyz")
}

/// `kPEPE` → `1000PEPE`, the multiplier other exchanges put in front.
pub fn base_of(name: &str) -> String {
    match name.strip_prefix('k') {
        Some(rest) if rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) => format!("1000{rest}"),
        _ => name.to_string(),
    }
}

/// Listed perpetuals: the coins, then trade.xyz's where its answer came. Prices
/// carry up to six decimals less the size decimals.
pub fn parse_products(body: &str, xyz: Option<&str>) -> Result<Vec<Product>, VenueError> {
    let mut list = parse_universe(body)?;
    // trade.xyz's own list is a nicety: without it the coins still list.
    list.extend(xyz.and_then(|b| parse_universe(b).ok()).unwrap_or_default());
    Ok(unique(list))
}

fn parse_universe(body: &str) -> Result<Vec<Product>, VenueError> {
    let v: Value = serde_json::from_str(body)?;
    let universe = v[0]["universe"].as_array().ok_or_else(|| shape("hyperliquid universe"))?;
    let ctxs = v[1].as_array().ok_or_else(|| shape("hyperliquid contexts"))?;
    Ok(universe
        .iter()
        .zip(ctxs)
        .filter(|(u, _)| u["isDelisted"] != true)
        .filter_map(|(u, c)| {
            let name = u["name"].as_str()?;
            let decimals = u["szDecimals"].as_u64().map(|s| 6u8.saturating_sub(s as u8));
            let (last, volume) = (num(c.get("markPx")), num(c.get("dayNtlVlm")));
            let Some((_, base)) = name.split_once(':') else {
                return product(Venue::HyperliquidPerp, &base_of(name), "USD", name, decimals, volume, last, change(num(c.get("prevDayPx")), last));
            };
            // A deployer's market nobody trades has no candles to draw: listed, it would chart nothing.
            volume.filter(|v| *v > 0.0)?;
            let mut p = product(Venue::HyperliquidPerp, base, "USD", name, decimals, volume, last, change(num(c.get("prevDayPx")), last))?;
            p.name = Some(xyz_name(base));
            Some(p)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured 2026-09-23, trimmed.
    #[test]
    fn coins_become_usd_markets() {
        let body = r#"[{"universe":[{"szDecimals":5,"name":"BTC","maxLeverage":40},{"szDecimals":0,"name":"kPEPE","maxLeverage":10},{"szDecimals":1,"name":"MATIC","isDelisted":true}]},
            [{"markPx":"86882.0","prevDayPx":"85262.0","dayNtlVlm":"2758297422.27"},{"markPx":"0.009","prevDayPx":"0.009","dayNtlVlm":"1"},{"markPx":"0.3"}]]"#;
        let p = parse_products(body, None).unwrap();
        assert_eq!(p.iter().map(|p| p.market.as_str()).collect::<Vec<_>>(), ["hyperliquid-perp:BTCUSD", "hyperliquid-perp:1000PEPEUSD"]);
        assert_eq!((p[0].native.as_deref(), p[0].price_decimals), (Some("BTC"), Some(1)));
        assert_eq!(p[1].native.as_deref(), Some("kPEPE"));
        assert_eq!(base_of("kSHIB"), "1000SHIB");
        assert_eq!(base_of("kas"), "kas");
    }

    #[test]
    fn trade_xyz_s_markets_list_after_the_coins() {
        let coins = r#"[{"universe":[{"szDecimals":5,"name":"BTC"}]},[{"markPx":"86882.0","prevDayPx":"85262.0","dayNtlVlm":"2758297422.27"}]]"#;
        let xyz = r#"[{"universe":[{"szDecimals":4,"name":"xyz:SP500"},{"szDecimals":2,"name":"xyz:VIX"}]},
            [{"markPx":"7660.7","prevDayPx":"7755.3","dayNtlVlm":"253422410.2"},{"markPx":"20.0","prevDayPx":"20.0","dayNtlVlm":"0.0"}]]"#;
        let p = parse_products(coins, Some(xyz)).unwrap();
        assert_eq!(p.iter().map(|p| p.market.as_str()).collect::<Vec<_>>(), ["hyperliquid-perp:BTCUSD", "hyperliquid-perp:SP500USD"], "a market nobody trades is left out");
        assert_eq!((p[1].native.as_deref(), p[1].base.as_str(), p[1].name.as_deref()), (Some("xyz:SP500"), "SP500", Some("S&P 500 · trade.xyz")));
        assert_eq!(parse_products(coins, Some("not json")).unwrap().len(), 1, "the coins without trade.xyz");
    }
}
