//! Binance spot and USDⓈ-M perpetuals.

use std::collections::HashMap;

use serde_json::Value;
use tradoor_codec::Period;
use tradoor_core::Interval;

use crate::catalog::{num, tick_decimals, Product};
use crate::kline::OwnedKline;
use crate::{shape, Kind, MarketRef, Venue, VenueError};

pub const SPOT_REST: &str = "https://api.binance.com";
/// Binance's public market-data mirror, for networks where api.binance.com is
/// unreachable. Same /api/v3 shapes.
pub const SPOT_REST_MIRROR: &str = "https://data-api.binance.vision";
pub const PERP_REST: &str = "https://fapi.binance.com";
pub const ARCHIVE: &str = "https://data.binance.vision";

pub fn default_rest(venue: Venue) -> &'static str {
    match venue.kind() {
        Kind::Spot => SPOT_REST,
        Kind::Perp => PERP_REST,
    }
}

pub fn max_klines(venue: Venue) -> u32 {
    match venue.kind() {
        Kind::Spot => 1000,
        Kind::Perp => 1500,
    }
}

pub fn klines_url(base: &str, market: &MarketRef, interval: Interval, start: Option<i64>, end: Option<i64>, limit: u32) -> String {
    let path = match market.venue.kind() {
        Kind::Spot => "/api/v3/klines",
        Kind::Perp => "/fapi/v1/klines",
    };
    let mut url = format!("{base}{path}?symbol={}&interval={}&limit={limit}", market.symbol, interval.as_str());
    if let Some(s) = start {
        url += &format!("&startTime={s}");
    }
    if let Some(e) = end {
        url += &format!("&endTime={e}");
    }
    url
}

fn api_error(v: &Value) -> Option<VenueError> {
    let obj = v.as_object()?;
    Some(VenueError::Api { code: obj.get("code")?.as_i64()?, message: obj.get("msg")?.as_str()?.to_string() })
}

/// `[[openTime, "o", "h", "l", "c", "v", closeTime, "quote", trades, "takerBase", "takerQuote", "0"], ...]`
pub fn parse_klines(body: &str) -> Result<Vec<OwnedKline>, VenueError> {
    let v: Value = serde_json::from_str(body)?;
    if let Some(err) = api_error(&v) {
        return Err(err);
    }
    let rows = v.as_array().ok_or_else(|| shape("klines: expected an array"))?;
    rows.iter()
        .map(|row| {
            let a = row.as_array().filter(|a| a.len() >= 6).ok_or_else(|| shape("kline: expected an array of at least 6"))?;
            let s = |i: usize| a.get(i).and_then(Value::as_str).map(str::to_string);
            let req = |i: usize| s(i).ok_or_else(|| shape(format!("kline field {i} is not a string")));
            Ok(OwnedKline {
                open_time: a[0].as_i64().ok_or_else(|| shape("kline open time"))?,
                open: req(1)?,
                high: req(2)?,
                low: req(3)?,
                close: req(4)?,
                volume: req(5)?,
                quote_volume: s(7),
                trades: a.get(8).and_then(Value::as_u64),
                taker_buy_volume: s(9),
            })
        })
        .collect()
}

/// A monthly or daily kline archive on data.binance.vision, or `None` for a
/// venue or period Binance does not archive.
pub fn archive_url(market: &MarketRef, interval: Interval, period: Period) -> Option<String> {
    let segment = match market.venue {
        Venue::BinanceSpot => "spot",
        Venue::BinancePerp => "futures/um",
        _ => return None,
    };
    let frequency = match period {
        Period::Month(..) => "monthly",
        Period::Day(..) => "daily",
        Period::Year(_) => return None,
    };
    let (sym, iv) = (&market.symbol, interval.as_str());
    Some(format!("{ARCHIVE}/data/{segment}/{frequency}/klines/{sym}/{iv}/{sym}-{iv}-{}.zip", period.name()))
}

/// The CSV inside an archive. Three dialects exist: futures files carry a header
/// row, older ones do not, and SPOT files from 2025 on stamp times in
/// MICROSECONDS. All three are read to milliseconds here.
pub fn parse_archive_csv(text: &str) -> Result<Vec<OwnedKline>, VenueError> {
    let mut out = Vec::new();
    for (n, line) in text.lines().map(str::trim).filter(|l| !l.is_empty()).enumerate() {
        let f: Vec<&str> = line.split(',').collect();
        let Ok(raw_time) = f[0].parse::<i64>() else {
            if n == 0 {
                continue; // the header
            }
            return Err(shape(format!("archive line {}: open time {:?}", n + 1, f[0])));
        };
        if f.len() < 11 {
            return Err(shape(format!("archive line {}: {} fields", n + 1, f.len())));
        }
        out.push(OwnedKline {
            open_time: if raw_time >= 100_000_000_000_000 { raw_time / 1000 } else { raw_time },
            open: f[1].to_string(),
            high: f[2].to_string(),
            low: f[3].to_string(),
            close: f[4].to_string(),
            volume: f[5].to_string(),
            quote_volume: Some(f[7].to_string()),
            trades: f[8].parse().ok(),
            taker_buy_volume: Some(f[9].to_string()),
        });
    }
    Ok(out)
}

/// `<sha256>  <file name>`.
pub fn parse_checksum(text: &str) -> Option<String> {
    let hash = text.split_whitespace().next()?;
    (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
}

pub fn exchange_info_url(base: &str, venue: Venue) -> String {
    match venue.kind() {
        Kind::Spot => format!("{base}/api/v3/exchangeInfo?permissions=SPOT"),
        Kind::Perp => format!("{base}/fapi/v1/exchangeInfo"),
    }
}

pub fn tickers_url(base: &str, venue: Venue) -> String {
    match venue.kind() {
        Kind::Spot => format!("{base}/api/v3/ticker/24hr"),
        Kind::Perp => format!("{base}/fapi/v1/ticker/24hr"),
    }
}

/// Tradable markets, joined with the 24h ticker table when one is given.
/// Futures list quarterly contracts beside the perpetual; only the perpetual is
/// kept — a chart of a contract that expires is a chart that stops.
pub fn parse_products(venue: Venue, info: &str, tickers: Option<&str>) -> Result<Vec<Product>, VenueError> {
    let info: Value = serde_json::from_str(info)?;
    if let Some(err) = api_error(&info) {
        return Err(err);
    }
    let stats: HashMap<String, Value> = match tickers {
        Some(body) => serde_json::from_str::<Value>(body)?
            .as_array()
            .map(|rows| rows.iter().filter_map(|r| Some((r.get("symbol")?.as_str()?.to_string(), r.clone()))).collect())
            .unwrap_or_default(),
        None => HashMap::new(),
    };
    let symbols = info.get("symbols").and_then(Value::as_array).ok_or_else(|| shape("exchangeInfo.symbols"))?;
    Ok(symbols
        .iter()
        .filter(|s| s.get("status").and_then(Value::as_str) == Some("TRADING"))
        .filter(|s| match venue.kind() {
            Kind::Spot => s.get("isSpotTradingAllowed").and_then(Value::as_bool) != Some(false),
            Kind::Perp => s.get("contractType").and_then(Value::as_str) == Some("PERPETUAL"),
        })
        .filter_map(|s| {
            let symbol = s.get("symbol")?.as_str()?;
            let market = MarketRef::new(venue, symbol)?;
            let tick = s.get("filters")?.as_array()?.iter().find(|f| f.get("filterType").and_then(Value::as_str) == Some("PRICE_FILTER")).and_then(|f| f.get("tickSize")?.as_str());
            let t = stats.get(symbol);
            Some(Product {
                market: market.to_string(),
                venue: venue.id(),
                symbol: symbol.to_string(),
                base: s.get("baseAsset")?.as_str()?.to_string(),
                quote: s.get("quoteAsset")?.as_str()?.to_string(),
                kind: venue.kind().as_str(),
                native: None,
                name: None,
                price_decimals: tick_decimals(tick),
                quote_volume_24h: t.and_then(|t| num(t.get("quoteVolume"))),
                last_price: t.and_then(|t| num(t.get("lastPrice"))),
                change_pct: t.and_then(|t| num(t.get("priceChangePercent"))),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tradoor_core::time::parse_iso;

    #[test]
    fn urls() {
        let perp = MarketRef::parse("binance-perp:BTCUSDT").unwrap();
        assert_eq!(klines_url(PERP_REST, &perp, Interval::M1, Some(1), None, 1500), "https://fapi.binance.com/fapi/v1/klines?symbol=BTCUSDT&interval=1m&limit=1500&startTime=1");
        assert_eq!(archive_url(&perp, Interval::M1, Period::Month(2021, 9)).unwrap(), "https://data.binance.vision/data/futures/um/monthly/klines/BTCUSDT/1m/BTCUSDT-1m-2021-09.zip");
        let spot = MarketRef::parse("binance-spot:BTCUSDT").unwrap();
        assert_eq!(archive_url(&spot, Interval::M1, Period::Day(2026, 9, 12)).unwrap(), "https://data.binance.vision/data/spot/daily/klines/BTCUSDT/1m/BTCUSDT-1m-2026-09-12.zip");
        assert!(archive_url(&MarketRef::parse("bybit-perp:BTCUSDT").unwrap(), Interval::M1, Period::Month(2026, 1)).is_none());
    }

    #[test]
    fn parses_rest_klines_and_errors() {
        let body = r#"[[1789171200000,"77191.10","77228.90","77191.00","77223.80","178.850",1789171259999,"13808634.94520",2223,"113.895","8793299.45060","0"]]"#;
        let k = parse_klines(body).unwrap();
        assert_eq!((k[0].open_time, k[0].close.as_str(), k[0].trades, k[0].taker_buy_volume.as_deref()), (1789171200000, "77223.80", Some(2223), Some("113.895")));
        assert_eq!(parse_klines(r#"{"code":-1121,"msg":"Invalid symbol."}"#), Err(VenueError::Api { code: -1121, message: "Invalid symbol.".into() }));
    }

    #[test]
    fn reads_all_three_archive_dialects() {
        let futures = "open_time,open,high,low,close,volume,close_time,quote_volume,count,taker_buy_volume,taker_buy_quote_volume,ignore\n1789171200000,77191.10,77228.90,77191.00,77223.80,178.850,1789171259999,13808634.94520,2223,113.895,8793299.45060,0\n";
        let spot_micros = "1789171200000000,77225.71000000,77254.71000000,77225.70000000,77254.70000000,5.24138000,1789171259999999,404870.60257790,1083,2.40428000,185700.90472210,0\n";
        let old = "1630454400000,47150.32,47173.55,47100.00,47103.92,409.935,1630454459999,19320643.11616,3612,162.740,7671862.74123,0\r\n";
        for (text, open_time) in [(futures, 1789171200000), (spot_micros, 1789171200000), (old, 1630454400000)] {
            let rows = parse_archive_csv(text).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].open_time, open_time);
        }
        let ticks = crate::kline::to_ticks(Interval::M1, &parse_archive_csv(spot_micros).unwrap()).unwrap();
        assert_eq!(ticks.open_time[0], parse_iso("2026-09-12").unwrap());
        assert_eq!(ticks.to_candles()[0].close, 77254.7);
        assert!(parse_archive_csv("1,2\n").is_err());
    }

    #[test]
    fn checksum_and_products() {
        assert_eq!(parse_checksum("3AF0e1d3ba1c7e34c8f8b1d5f7c7a46bb7e6a1b0c6d5e4f3a2b1c0d9e8f7a6b5  BTCUSDT-1m-2026-09-12.zip").unwrap().len(), 64);
        assert_eq!(parse_checksum("<html>"), None);

        let info = r#"{"symbols":[
            {"symbol":"BTCUSDT","status":"TRADING","contractType":"PERPETUAL","baseAsset":"BTC","quoteAsset":"USDT","filters":[{"filterType":"PRICE_FILTER","tickSize":"0.10"}]},
            {"symbol":"BTCUSDT_261225","status":"TRADING","contractType":"CURRENT_QUARTER","baseAsset":"BTC","quoteAsset":"USDT","filters":[]},
            {"symbol":"OLDUSDT","status":"SETTLING","contractType":"PERPETUAL","baseAsset":"OLD","quoteAsset":"USDT","filters":[]}]}"#;
        let tickers = r#"[{"symbol":"BTCUSDT","lastPrice":"77223.8","priceChangePercent":"-1.25","quoteVolume":"12345678.9"}]"#;
        let products = parse_products(Venue::BinancePerp, info, Some(tickers)).unwrap();
        assert_eq!(products.len(), 1);
        let p = &products[0];
        assert_eq!((p.market.as_str(), p.price_decimals, p.last_price, p.change_pct), ("binance-perp:BTCUSDT", Some(1), Some(77223.8), Some(-1.25)));
    }
}
