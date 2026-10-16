//! Bitget spot and USDT-margined perpetuals. Bitget's names are already
//! `BTCUSDT`, so no native name is carried.

use std::collections::HashMap;

use serde_json::Value;

use crate::catalog::{num, product, unique, Product};
use crate::{shape, Kind, Venue, VenueError};

pub const REST: &str = "https://api.bitget.com";

pub fn instruments_url(venue: Venue) -> String {
    match venue.kind() {
        Kind::Spot => format!("{REST}/api/v2/spot/public/symbols"),
        Kind::Perp => format!("{REST}/api/v2/mix/market/contracts?productType=usdt-futures"),
    }
}

pub fn tickers_url(venue: Venue) -> String {
    match venue.kind() {
        Kind::Spot => format!("{REST}/api/v2/spot/market/tickers"),
        Kind::Perp => format!("{REST}/api/v2/mix/market/tickers?productType=usdt-futures"),
    }
}

fn data(body: &str) -> Result<Vec<Value>, VenueError> {
    let v: Value = serde_json::from_str(body)?;
    if v["code"] != "00000" {
        return Err(VenueError::Api { code: v["code"].as_str().and_then(|c| c.parse().ok()).unwrap_or(-1), message: v["msg"].as_str().unwrap_or_default().to_string() });
    }
    v["data"].as_array().cloned().ok_or_else(|| shape("bitget data"))
}

/// Tradable markets. Bitget's `change24h` is a fraction.
pub fn parse_products(venue: Venue, instruments: &str, tickers: Option<&str>) -> Result<Vec<Product>, VenueError> {
    let stats: HashMap<String, Value> = match tickers {
        Some(t) => data(t)?.into_iter().filter_map(|r| Some((r["symbol"].as_str()?.to_string(), r))).collect(),
        None => HashMap::new(),
    };
    let perp = venue.kind() == Kind::Perp;
    let list = data(instruments)?
        .iter()
        .filter(|i| if perp { i["symbolStatus"] == "normal" && i["symbolType"] == "perpetual" } else { i["status"] == "online" })
        .filter_map(|i| {
            let symbol = i["symbol"].as_str()?;
            let decimals = if perp { i["pricePlace"].as_str() } else { i["pricePrecision"].as_str() }.and_then(|d| d.parse().ok());
            let t = stats.get(symbol);
            // Its stock tokens ("rSPY") report the stock's own turnover on its
            // exchange — twenty billion dollars a day — not what trades on
            // Bitget, and topped every list sorted by it. No figure beats that one.
            let stock = i["areaSymbol"] == "yes";
            let volume = t.and_then(|t| num(t.get("quoteVolume"))).filter(|_| !stock);
            product(venue, i["baseCoin"].as_str()?, i["quoteCoin"].as_str()?, symbol, decimals, volume, t.and_then(|t| num(t.get("lastPr"))), t.and_then(|t| num(t.get("change24h"))).map(|c| c * 100.0))
        })
        .collect();
    Ok(unique(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured 2026-09-23, trimmed.
    #[test]
    fn perpetuals_and_spot() {
        let contracts = r#"{"code":"00000","msg":"success","data":[
            {"symbol":"BTCUSDT","baseCoin":"BTC","quoteCoin":"USDT","pricePlace":"1","symbolType":"perpetual","symbolStatus":"normal"},
            {"symbol":"BTCUSDT0926","baseCoin":"BTC","quoteCoin":"USDT","pricePlace":"1","symbolType":"delivery","symbolStatus":"normal"}]}"#;
        let tickers = r#"{"code":"00000","data":[{"symbol":"BTCUSDT","lastPr":"86558.3","change24h":"0.01182","quoteVolume":"2681837768.8841"}]}"#;
        let p = parse_products(Venue::BitgetPerp, contracts, Some(tickers)).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].market.as_str(), p[0].native.as_deref(), p[0].price_decimals), ("bitget-perp:BTCUSDT", None, Some(1)));
        assert!((p[0].change_pct.unwrap() - 1.182).abs() < 1e-9);
        let spot = r#"{"code":"00000","data":[{"symbol":"BTCUSDT","baseCoin":"BTC","quoteCoin":"USDT","pricePrecision":"2","status":"online"},{"symbol":"OLDUSDT","baseCoin":"OLD","quoteCoin":"USDT","pricePrecision":"2","status":"offline"}]}"#;
        assert_eq!(parse_products(Venue::BitgetSpot, spot, None).unwrap().len(), 1);
    }

    #[test]
    fn a_stock_token_has_no_turnover_of_its_own() {
        let spot = r#"{"code":"00000","data":[
            {"symbol":"RSPYUSDT","baseCoin":"rSPY","quoteCoin":"USDT","pricePrecision":"2","status":"online","areaSymbol":"yes"},
            {"symbol":"BTCUSDT","baseCoin":"BTC","quoteCoin":"USDT","pricePrecision":"2","status":"online","areaSymbol":"no"}]}"#;
        let tickers = r#"{"code":"00000","data":[
            {"symbol":"RSPYUSDT","lastPr":"773.17","change24h":"0.0077","quoteVolume":"20051277094.0366"},
            {"symbol":"BTCUSDT","lastPr":"82106.86","change24h":"0.01","quoteVolume":"316840328.28"}]}"#;
        let p = parse_products(Venue::BitgetSpot, spot, Some(tickers)).unwrap();
        let of = |s: &str| p.iter().find(|p| p.symbol == s).unwrap();
        assert_eq!(of("RSPYUSDT").quote_volume_24h, None, "the stock's own turnover is not Bitget's");
        assert_eq!(of("RSPYUSDT").last_price, Some(773.17));
        assert!(of("BTCUSDT").quote_volume_24h.is_some());
    }
}
