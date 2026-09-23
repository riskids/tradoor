//! OKX spot and USDT/USDC-margined perpetual swaps.
//!
//! OKX names markets `BTC-USDT` and `BTC-USDT-SWAP`; here they are
//! `okx-spot:BTCUSDT` and `okx-perp:BTCUSDT`, with the OKX name as `native`.

use std::collections::HashMap;

use serde_json::Value;

use crate::catalog::{change, num, product, tick_decimals, unique, Product};
use crate::{shape, Kind, Venue, VenueError};

pub const REST: &str = "https://www.okx.com";

fn inst_type(venue: Venue) -> &'static str {
    match venue.kind() {
        Kind::Spot => "SPOT",
        Kind::Perp => "SWAP",
    }
}

pub fn instruments_url(venue: Venue) -> String {
    format!("{REST}/api/v5/public/instruments?instType={}", inst_type(venue))
}

pub fn tickers_url(venue: Venue) -> String {
    format!("{REST}/api/v5/market/tickers?instType={}", inst_type(venue))
}

fn data(body: &str) -> Result<Vec<Value>, VenueError> {
    let v: Value = serde_json::from_str(body)?;
    if v["code"] != "0" {
        return Err(VenueError::Api { code: v["code"].as_str().and_then(|c| c.parse().ok()).unwrap_or(-1), message: v["msg"].as_str().unwrap_or_default().to_string() });
    }
    v["data"].as_array().cloned().ok_or_else(|| shape("okx data"))
}

/// Live markets, with the ticker table's last price and 24h change. A swap's
/// `volCcy24h` is in coins, a spot market's in the quote.
pub fn parse_products(venue: Venue, instruments: &str, tickers: Option<&str>) -> Result<Vec<Product>, VenueError> {
    let stats: HashMap<String, Value> = match tickers {
        Some(t) => data(t)?.into_iter().filter_map(|r| Some((r["instId"].as_str()?.to_string(), r))).collect(),
        None => HashMap::new(),
    };
    let perp = venue.kind() == Kind::Perp;
    let list = data(instruments)?
        .iter()
        .filter(|i| i["state"] == "live")
        .filter(|i| !perp || (i["ctType"] == "linear" && matches!(i["settleCcy"].as_str(), Some("USDT" | "USDC"))))
        .filter_map(|i| {
            let native = i["instId"].as_str()?;
            let (base, quote) = if perp { i["instFamily"].as_str()?.split_once('-')? } else { (i["baseCcy"].as_str()?, i["quoteCcy"].as_str()?) };
            let t = stats.get(native);
            let last = t.and_then(|t| num(t.get("last")));
            let volume = t.and_then(|t| num(t.get("volCcy24h"))).map(|v| if perp { v * last.unwrap_or(0.0) } else { v });
            product(venue, base, quote, native, tick_decimals(i["tickSz"].as_str()), volume, last, change(t.and_then(|t| num(t.get("open24h"))), last))
        })
        .collect();
    Ok(unique(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured 2026-09-23, trimmed.
    #[test]
    fn swaps_and_spot_are_named_by_coin_and_quote() {
        let swaps = r#"{"code":"0","data":[
            {"instId":"BTC-USDT-SWAP","ctType":"linear","ctVal":"0.01","settleCcy":"USDT","instFamily":"BTC-USDT","tickSz":"0.1","state":"live"},
            {"instId":"BTC-USD-SWAP","ctType":"inverse","ctVal":"100","settleCcy":"BTC","instFamily":"BTC-USD","tickSz":"0.1","state":"live"},
            {"instId":"NEW-USDT-SWAP","ctType":"linear","settleCcy":"USDT","instFamily":"NEW-USDT","tickSz":"0.001","state":"preopen"}],"msg":""}"#;
        let tickers = r#"{"code":"0","data":[{"instId":"BTC-USDT-SWAP","last":"86000","open24h":"85000","volCcy24h":"100"}],"msg":""}"#;
        let p = parse_products(Venue::OkxPerp, swaps, Some(tickers)).unwrap();
        assert_eq!(p.len(), 1, "inverse and pre-open swaps are left out");
        assert_eq!((p[0].market.as_str(), p[0].native.as_deref(), p[0].price_decimals), ("okx-perp:BTCUSDT", Some("BTC-USDT-SWAP"), Some(1)));
        assert_eq!(p[0].quote_volume_24h, Some(8_600_000.0), "a swap's volume is in coins");
        assert!((p[0].change_pct.unwrap() - 1.1765).abs() < 1e-3);

        let spot = r#"{"code":"0","data":[{"instId":"BTC-USDT","baseCcy":"BTC","quoteCcy":"USDT","tickSz":"0.1","state":"live"}],"msg":""}"#;
        let p = parse_products(Venue::OkxSpot, spot, None).unwrap();
        assert_eq!((p[0].market.as_str(), p[0].native.as_deref()), ("okx-spot:BTCUSDT", Some("BTC-USDT")));
        assert!(parse_products(Venue::OkxSpot, r#"{"code":"50011","msg":"Too Many Requests","data":[]}"#, None).is_err());
    }
}
