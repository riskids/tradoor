//! Bybit v5 spot and linear perpetuals.

use std::collections::HashMap;

use serde_json::Value;
use tradoor_core::Interval;

use crate::catalog::{num, tick_decimals, Product};
use crate::kline::OwnedKline;
use crate::{shape, Kind, MarketRef, Venue, VenueError};

pub const REST: &str = "https://api.bybit.com";
pub const MAX_KLINES: u32 = 1000;

fn category(venue: Venue) -> &'static str {
    match venue.kind() {
        Kind::Spot => "spot",
        Kind::Perp => "linear",
    }
}

pub fn klines_url(base: &str, market: &MarketRef, interval: Interval, start: Option<i64>, end: Option<i64>, limit: u32) -> String {
    let mut url = format!("{base}/v5/market/kline?category={}&symbol={}&interval={}&limit={limit}", category(market.venue), market.symbol, interval.bybit());
    if let Some(s) = start {
        url += &format!("&start={s}");
    }
    if let Some(e) = end {
        url += &format!("&end={e}");
    }
    url
}

/// Unwraps `{retCode, retMsg, result}`.
fn result(body: &str) -> Result<Value, VenueError> {
    let mut v: Value = serde_json::from_str(body)?;
    let code = v.get("retCode").and_then(Value::as_i64).ok_or_else(|| shape("retCode"))?;
    if code != 0 {
        let message = v.get("retMsg").and_then(Value::as_str).unwrap_or_default().to_string();
        return Err(VenueError::Api { code, message });
    }
    Ok(v.get_mut("result").map(Value::take).unwrap_or(Value::Null))
}

/// `result.list` is NEWEST FIRST; returned oldest first, which is what every
/// consumer assumes. Bybit sends no taker volume or trade count.
pub fn parse_klines(body: &str) -> Result<Vec<OwnedKline>, VenueError> {
    let result = result(body)?;
    let list = result.get("list").and_then(Value::as_array).ok_or_else(|| shape("kline result.list"))?;
    let mut out: Vec<OwnedKline> = list
        .iter()
        .map(|row| {
            let a = row.as_array().filter(|a| a.len() >= 6).ok_or_else(|| shape("kline row"))?;
            let s = |i: usize| a.get(i).and_then(Value::as_str).map(str::to_string).ok_or_else(|| shape(format!("kline field {i}")));
            Ok(OwnedKline {
                open_time: s(0)?.parse().map_err(|_| shape("kline start"))?,
                open: s(1)?,
                high: s(2)?,
                low: s(3)?,
                close: s(4)?,
                volume: s(5)?,
                quote_volume: s(6).ok(),
                taker_buy_volume: None,
                trades: None,
            })
        })
        .collect::<Result<_, VenueError>>()?;
    out.sort_by_key(|k| k.open_time);
    Ok(out)
}

pub fn instruments_url(base: &str, venue: Venue, cursor: Option<&str>) -> String {
    let mut url = format!("{base}/v5/market/instruments-info?category={}&limit=1000", category(venue));
    if let Some(c) = cursor.filter(|c| !c.is_empty()) {
        url += &format!("&cursor={c}");
    }
    url
}

pub fn tickers_url(base: &str, venue: Venue) -> String {
    format!("{base}/v5/market/tickers?category={}", category(venue))
}

/// One page of instruments and the cursor for the next, if any.
pub fn parse_instruments(venue: Venue, body: &str, tickers: Option<&str>) -> Result<(Vec<Product>, Option<String>), VenueError> {
    let result = result(body)?;
    let stats: HashMap<String, Value> = match tickers {
        Some(t) => result_list(t)?.into_iter().filter_map(|r| Some((r.get("symbol")?.as_str()?.to_string(), r))).collect(),
        None => HashMap::new(),
    };
    let list = result.get("list").and_then(Value::as_array).ok_or_else(|| shape("instruments result.list"))?;
    let products = list
        .iter()
        .filter(|s| s.get("status").and_then(Value::as_str) == Some("Trading"))
        .filter(|s| venue.kind() == Kind::Spot || s.get("contractType").and_then(Value::as_str) == Some("LinearPerpetual"))
        .filter_map(|s| {
            let symbol = s.get("symbol")?.as_str()?;
            let market = MarketRef::new(venue, symbol)?;
            let t = stats.get(symbol);
            Some(Product {
                market: market.to_string(),
                venue: venue.id(),
                symbol: symbol.to_string(),
                base: s.get("baseCoin")?.as_str()?.to_string(),
                quote: s.get("quoteCoin")?.as_str()?.to_string(),
                kind: venue.kind().as_str(),
                price_decimals: tick_decimals(s.get("priceFilter").and_then(|f| f.get("tickSize")?.as_str())),
                quote_volume_24h: t.and_then(|t| num(t.get("turnover24h"))),
                last_price: t.and_then(|t| num(t.get("lastPrice"))),
                // Bybit reports a FRACTION; Binance a percent. One unit here.
                change_pct: t.and_then(|t| num(t.get("price24hPcnt"))).map(|v| v * 100.0),
            })
        })
        .collect();
    let cursor = result.get("nextPageCursor").and_then(Value::as_str).filter(|c| !c.is_empty()).map(str::to_string);
    Ok((products, cursor))
}

fn result_list(body: &str) -> Result<Vec<Value>, VenueError> {
    let result = result(body)?;
    Ok(result.get("list").and_then(Value::as_array).cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn klines_come_back_oldest_first() {
        let body = r#"{"retCode":0,"retMsg":"OK","result":{"category":"linear","symbol":"BTCUSDT","list":[
            ["1789171260000","77223.8","77252.2","77170.3","77235.4","365.77","28243602.8947"],
            ["1789171200000","77191.1","77228.9","77191","77223.8","178.85","13808634.9452"]]}}"#;
        let k = parse_klines(body).unwrap();
        assert_eq!(k.iter().map(|k| k.open_time).collect::<Vec<_>>(), [1789171200000, 1789171260000]);
        assert_eq!(k[0].quote_volume.as_deref(), Some("13808634.9452"));
        assert_eq!(parse_klines(r#"{"retCode":10001,"retMsg":"params error","result":{}}"#), Err(VenueError::Api { code: 10001, message: "params error".into() }));
        let m = MarketRef::parse("bybit-perp:BTCUSDT").unwrap();
        assert_eq!(klines_url(REST, &m, Interval::H4, None, Some(5), 1000), "https://api.bybit.com/v5/market/kline?category=linear&symbol=BTCUSDT&interval=240&limit=1000&end=5");
    }

    #[test]
    fn instruments_keep_perpetuals_and_normalise_change() {
        let body = r#"{"retCode":0,"retMsg":"OK","result":{"list":[
            {"symbol":"BTCUSDT","status":"Trading","baseCoin":"BTC","quoteCoin":"USDT","contractType":"LinearPerpetual","priceFilter":{"tickSize":"0.10"}},
            {"symbol":"BTC-26DEC25","status":"Trading","baseCoin":"BTC","quoteCoin":"USDT","contractType":"LinearFutures","priceFilter":{"tickSize":"0.50"}}],
            "nextPageCursor":"abc"}}"#;
        let tickers = r#"{"retCode":0,"retMsg":"OK","result":{"list":[{"symbol":"BTCUSDT","lastPrice":"77000","price24hPcnt":"0.0629","turnover24h":"1000"}]}}"#;
        let (products, cursor) = parse_instruments(Venue::BybitPerp, body, Some(tickers)).unwrap();
        assert_eq!(products.len(), 1);
        assert!((products[0].change_pct.unwrap() - 6.29).abs() < 1e-9);
        assert_eq!(cursor.as_deref(), Some("abc"));
    }
}
