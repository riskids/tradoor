//! Lighter perpetuals. Markets are numbered; the number is the native name.

use serde_json::Value;

use crate::catalog::{num, product, unique, Product};
use crate::{shape, Venue, VenueError};

pub const REST: &str = "https://mainnet.zklighter.elliot.ai";

pub fn details_url() -> String {
    format!("{REST}/api/v1/orderBookDetails")
}

/// Active perpetuals, priced in USD. `daily_price_change` is a percent.
pub fn parse_products(body: &str) -> Result<Vec<Product>, VenueError> {
    let v: Value = serde_json::from_str(body)?;
    let list = v["order_book_details"]
        .as_array()
        .ok_or_else(|| shape("lighter order_book_details"))?
        .iter()
        .filter(|o| o["status"] == "active" && o["market_type"] == "perp")
        .filter_map(|o| {
            let id = o["market_id"].as_u64()?;
            product(
                Venue::LighterPerp,
                o["symbol"].as_str()?,
                "USD",
                &id.to_string(),
                o["price_decimals"].as_u64().map(|d| d as u8),
                num(o.get("daily_quote_token_volume")),
                num(o.get("last_trade_price")),
                num(o.get("daily_price_change")),
            )
        })
        .collect();
    Ok(unique(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured 2026-09-23, trimmed.
    #[test]
    fn markets_carry_their_number() {
        let body = r#"{"code":200,"order_book_details":[
            {"symbol":"BTC","market_id":1,"status":"active","market_type":"perp","last_trade_price":86600.1,"daily_price_change":1.2,"daily_quote_token_volume":1000.0,"price_decimals":1},
            {"symbol":"PLTR","market_id":111,"status":"inactive","market_type":"perp","price_decimals":3}],"spot_order_book_details":[]}"#;
        let p = parse_products(body).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].market.as_str(), p[0].native.as_deref(), p[0].change_pct), ("lighter-perp:BTCUSD", Some("1"), Some(1.2)));
    }
}
