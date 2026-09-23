//! Kraken spot and Kraken Futures' multi-collateral perpetuals.
//!
//! Spot pairs are named as Kraken's v2 API writes them, `BTC/USD` — the REST
//! OHLC endpoint takes that name too. Perpetuals are `PF_XBTUSD`. Both are
//! `…:BTCUSD` here.

use std::collections::HashMap;

use serde_json::Value;

use crate::catalog::{change, decimals_of, num, product, unique, Product};
use crate::{shape, Venue, VenueError};

pub const SPOT: &str = "https://api.kraken.com";
pub const FUTURES: &str = "https://futures.kraken.com";
/// Quotes a spot pair is listed under here.
const SPOT_QUOTES: [&str; 4] = ["USD", "USDT", "USDC", "EUR"];

pub fn spot_pairs_url() -> String {
    format!("{SPOT}/0/public/AssetPairs")
}

pub fn spot_tickers_url() -> String {
    format!("{SPOT}/0/public/Ticker")
}

pub fn futures_instruments_url() -> String {
    format!("{FUTURES}/derivatives/api/v3/instruments")
}

pub fn futures_tickers_url() -> String {
    format!("{FUTURES}/derivatives/api/v3/tickers")
}

/// Kraken's old names for two coins; its v2 API uses the usual ones.
fn coin(name: &str) -> &str {
    match name {
        "XBT" => "BTC",
        "XDG" => "DOGE",
        other => other,
    }
}

fn result(body: &str) -> Result<Value, VenueError> {
    let mut v: Value = serde_json::from_str(body)?;
    if let Some(e) = v["error"].as_array().and_then(|e| e.first()) {
        return Err(VenueError::Api { code: -1, message: e.as_str().unwrap_or_default().to_string() });
    }
    Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null))
}

/// Online spot pairs quoted in the dollar, its stablecoins or the euro.
/// Tickers are keyed by Kraken's pair name; `o` is today's open.
pub fn parse_spot(pairs: &str, tickers: Option<&str>) -> Result<Vec<Product>, VenueError> {
    let stats = match tickers {
        Some(t) => result(t)?,
        None => Value::Null,
    };
    let pairs = result(pairs)?;
    let pairs = pairs.as_object().ok_or_else(|| shape("kraken pairs"))?;
    let list = pairs
        .iter()
        .filter(|(_, p)| p["status"] == "online")
        .filter_map(|(key, p)| {
            let (base, quote) = p["wsname"].as_str()?.split_once('/')?;
            let (base, quote) = (coin(base), coin(quote));
            if !SPOT_QUOTES.contains(&quote) {
                return None;
            }
            let t = &stats[key];
            let last = num(t["c"].get(0));
            let volume = num(t["v"].get(1)).zip(last).map(|(v, l)| v * l);
            product(Venue::KrakenSpot, base, quote, &format!("{base}/{quote}"), p["pair_decimals"].as_u64().map(|d| d as u8), volume, last, change(num(t.get("o")), last))
        })
        .collect();
    Ok(unique(list))
}

/// Tradeable perpetuals (`PF_…`).
pub fn parse_futures(instruments: &str, tickers: Option<&str>) -> Result<Vec<Product>, VenueError> {
    let stats: HashMap<String, Value> = match tickers {
        Some(t) => serde_json::from_str::<Value>(t)?["tickers"].as_array().map(|rows| rows.iter().filter_map(|r| Some((r["symbol"].as_str()?.to_string(), r.clone()))).collect()).unwrap_or_default(),
        None => HashMap::new(),
    };
    let v: Value = serde_json::from_str(instruments)?;
    let list = v["instruments"]
        .as_array()
        .ok_or_else(|| shape("kraken instruments"))?
        .iter()
        .filter(|i| i["tradeable"] == true && i["type"] == "flexible_futures")
        .filter_map(|i| {
            let symbol = i["symbol"].as_str()?;
            symbol.strip_prefix("PF_")?;
            let (base, quote) = (coin(i["base"].as_str()?), i["quote"].as_str()?);
            let t = stats.get(symbol);
            let last = t.and_then(|t| num(t.get("last")));
            product(Venue::KrakenPerp, base, quote, symbol, num(i.get("tickSize")).and_then(decimals_of), t.and_then(|t| num(t.get("volumeQuote"))), last, change(t.and_then(|t| num(t.get("open24h"))), last))
        })
        .collect();
    Ok(unique(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured 2026-09-23, trimmed.
    #[test]
    fn spot_pairs_use_the_v2_names() {
        let pairs = r#"{"error":[],"result":{
            "XXBTZUSD":{"altname":"XBTUSD","wsname":"XBT/USD","base":"XXBT","quote":"ZUSD","pair_decimals":1,"status":"online"},
            "XDGUSD":{"altname":"XDGUSD","wsname":"XDG/USD","pair_decimals":7,"status":"online"},
            "XXBTZJPY":{"altname":"XBTJPY","wsname":"XBT/JPY","pair_decimals":0,"status":"online"}}}"#;
        let tickers = r#"{"error":[],"result":{"XXBTZUSD":{"c":["86583.8","0.35"],"v":["511.3","2948.9"],"o":"86199.0"}}}"#;
        let p = parse_spot(pairs, Some(tickers)).unwrap();
        let btc = p.iter().find(|p| p.symbol == "BTCUSD").unwrap();
        assert_eq!((btc.market.as_str(), btc.native.as_deref(), btc.price_decimals), ("kraken-spot:BTCUSD", Some("BTC/USD"), Some(1)));
        assert!((btc.quote_volume_24h.unwrap() - 2948.9 * 86583.8).abs() < 1.0);
        assert!(p.iter().any(|p| p.native.as_deref() == Some("DOGE/USD")));
        assert!(!p.iter().any(|p| p.quote == "JPY"));
    }

    #[test]
    fn futures_keep_the_perpetuals() {
        let instruments = r#"{"result":"success","instruments":[
            {"symbol":"PI_XBTUSD","type":"futures_inverse","tradeable":true,"tickSize":0.5},
            {"symbol":"PF_XBTUSD","type":"flexible_futures","base":"BTC","quote":"USD","tradeable":true,"tickSize":1},
            {"symbol":"PF_ETHUSD","type":"flexible_futures","base":"ETH","quote":"USD","tradeable":true,"tickSize":0.1}]}"#;
        let tickers = r#"{"tickers":[{"symbol":"PF_XBTUSD","last":86591,"open24h":85547,"volumeQuote":440322339.7}]}"#;
        let p = parse_futures(instruments, Some(tickers)).unwrap();
        assert_eq!(p.iter().map(|p| p.market.as_str()).collect::<Vec<_>>(), ["kraken-perp:BTCUSD", "kraken-perp:ETHUSD"]);
        assert_eq!((p[0].native.as_deref(), p[0].price_decimals, p[1].price_decimals), (Some("PF_XBTUSD"), Some(0), Some(1)));
        assert_eq!(p[0].last_price, Some(86591.0));
    }
}
