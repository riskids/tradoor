//! The exchanges' public trades: where they come from, how to ask, and how to
//! read one. Two things here listen — the alert watcher for the price, the flow
//! collector for which side was the aggressor — so the wire talk lives in one
//! place and both see the same trade.

use serde_json::Value;
use tradoor_venues::Venue;

/// The public streams each venue's trades come on, best first. A venue with
/// more than one is tried in turn: an address that answers but then says
/// nothing is as useless as one that refuses, and which is which moves with the
/// network — the page carries the same lists for the same reason.
pub fn stream_urls(venue: Venue) -> &'static [&'static str] {
    match venue {
        Venue::BinanceSpot => &["wss://stream.binance.com:9443/ws", "wss://data-stream.binance.vision/ws"],
        Venue::BinancePerp => &["wss://fstream.binance.com/market/ws", "wss://fstream.binance.com/ws"],
        Venue::BybitSpot => &["wss://stream.bybit.com/v5/public/spot"],
        Venue::BybitPerp => &["wss://stream.bybit.com/v5/public/linear"],
        Venue::OkxSpot | Venue::OkxPerp => &["wss://ws.okx.com:8443/ws/v5/public"],
        Venue::HyperliquidPerp => &["wss://api.hyperliquid.xyz/ws"],
        _ => &[],
    }
}

/// Whether a venue calls its markets something other than we do, so the names
/// asked for and the names answered with have to be translated.
pub fn renames(venue: Venue) -> bool {
    matches!(venue, Venue::OkxSpot | Venue::OkxPerp | Venue::HyperliquidPerp)
}

/// What to send to ask for these markets' trades, in the exchange's own names.
/// Most take one frame for the lot; Hyperliquid takes one subscription each.
pub fn subscribe(venue: Venue, symbols: &[String]) -> Vec<String> {
    match venue {
        Venue::BybitSpot | Venue::BybitPerp => {
            let args: Vec<String> = symbols.iter().map(|s| format!("\"publicTrade.{s}\"")).collect();
            vec![format!("{{\"op\":\"subscribe\",\"args\":[{}]}}", args.join(","))]
        }
        Venue::OkxSpot | Venue::OkxPerp => {
            let args: Vec<String> = symbols.iter().map(|s| format!("{{\"channel\":\"trades\",\"instId\":\"{s}\"}}")).collect();
            vec![format!("{{\"op\":\"subscribe\",\"args\":[{}]}}", args.join(","))]
        }
        Venue::HyperliquidPerp => symbols.iter().map(|s| format!("{{\"method\":\"subscribe\",\"subscription\":{{\"type\":\"trades\",\"coin\":\"{s}\"}}}}")).collect(),
        _ => {
            let params: Vec<String> = symbols.iter().map(|s| format!("\"{}@aggTrade\"", s.to_lowercase())).collect();
            vec![format!("{{\"method\":\"SUBSCRIBE\",\"params\":[{}],\"id\":1}}", params.join(","))]
        }
    }
}

/// What keeps a socket open where the exchange asks for it. Binance does not.
pub fn ping(venue: Venue) -> Option<&'static str> {
    match venue {
        Venue::BybitSpot | Venue::BybitPerp => Some("{\"op\":\"ping\"}"),
        // OKX wants the bare word, and closes a socket that says nothing for 30 seconds.
        Venue::OkxSpot | Venue::OkxPerp => Some("ping"),
        Venue::HyperliquidPerp => Some("{\"method\":\"ping\"}"),
        _ => None,
    }
}

/// One trade as both readers need it: what it cost, how much, and whether the
/// aggressor was buying — the taker's side, which is what flow counts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Print<'a> {
    pub symbol: &'a str,
    pub price: f64,
    pub qty: f64,
    pub buy: bool,
    pub at: i64,
}

impl Print<'_> {
    /// What it was worth, in the quote currency.
    pub fn usd(&self) -> f64 {
        self.price * self.qty
    }
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => s.parse().ok(),
        _ => v.as_f64(),
    }
    .filter(|p: &f64| p.is_finite() && *p > 0.0)
}

/// The trades in one frame. Anything else — a subscription answer, a heartbeat
/// — carries no trade and is skipped. The frame is parsed into `into` so the
/// borrowed symbols stay alive as long as the caller needs them.
pub fn read_prints<'a>(venue: Venue, frame: &'a Value, into: &mut Vec<Print<'a>>) {
    into.clear();
    match venue {
        Venue::OkxSpot | Venue::OkxPerp => {
            if frame["arg"]["channel"] != "trades" {
                return;
            }
            let Some(rows) = frame["data"].as_array() else { return };
            for r in rows {
                let (Some(symbol), Some(price), Some(qty)) = (r["instId"].as_str(), num(&r["px"]), num(&r["sz"])) else { continue };
                into.push(Print { symbol, price, qty, buy: r["side"] == "buy", at: r["ts"].as_str().and_then(|t| t.parse().ok()).unwrap_or_default() });
            }
        }
        Venue::HyperliquidPerp => {
            if frame["channel"] != "trades" {
                return;
            }
            let Some(rows) = frame["data"].as_array() else { return };
            for r in rows {
                let (Some(symbol), Some(price), Some(qty)) = (r["coin"].as_str(), num(&r["px"]), num(&r["sz"])) else { continue };
                // `B` is the aggressor buying, `A` taking the ask away.
                into.push(Print { symbol, price, qty, buy: r["side"] == "B", at: r["time"].as_i64().unwrap_or_default() });
            }
        }
        Venue::BybitSpot | Venue::BybitPerp => {
            if !frame["topic"].as_str().is_some_and(|t| t.starts_with("publicTrade.")) {
                return;
            }
            let Some(rows) = frame["data"].as_array() else { return };
            for r in rows {
                let (Some(symbol), Some(price), Some(qty)) = (r["s"].as_str(), num(&r["p"]), num(&r["v"])) else { continue };
                into.push(Print { symbol, price, qty, buy: r["S"] == "Buy", at: r["T"].as_i64().unwrap_or_default() });
            }
        }
        _ => {
            if frame["e"] != "aggTrade" {
                return;
            }
            let (Some(symbol), Some(price), Some(qty)) = (frame["s"].as_str(), num(&frame["p"]), num(&frame["q"])) else { return };
            // `m` is whether the BUYER was the maker, so the taker bought when it is false.
            into.push(Print { symbol, price, qty, buy: frame["m"] == false, at: frame["T"].as_i64().unwrap_or_default() });
        }
    }
}

/// The trades in one frame, allocated — for a caller with no buffer to lend.
pub fn prints(venue: Venue, text: &str) -> Vec<(String, f64, f64, bool)> {
    let Ok(frame) = serde_json::from_str::<Value>(text) else { return Vec::new() };
    let mut buffer = Vec::new();
    read_prints(venue, &frame, &mut buffer);
    buffer.iter().map(|p| (p.symbol.to_string(), p.price, p.qty, p.buy)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trade_says_its_size_and_which_side_took_it() {
        let binance = r#"{"e":"aggTrade","E":1,"s":"BTCUSDT","p":"84250.10","q":"0.5","m":false,"T":7}"#;
        assert_eq!(prints(Venue::BinancePerp, binance), [("BTCUSDT".to_string(), 84_250.10, 0.5, true)], "the buyer took it");
        let sold = binance.replace("\"m\":false", "\"m\":true");
        assert_eq!(prints(Venue::BinancePerp, &sold)[0].3, false, "the buyer made it, so the seller took it");
        assert!(prints(Venue::BinancePerp, r#"{"result":null,"id":1}"#).is_empty());

        let bybit = r#"{"topic":"publicTrade.BTCUSDT","data":[{"s":"BTCUSDT","p":"84251.5","v":"0.2","S":"Buy","T":9},{"s":"BTCUSDT","p":"84252","v":"0.1","S":"Sell","T":9}]}"#;
        let read = prints(Venue::BybitSpot, bybit);
        assert_eq!(read.len(), 2);
        assert_eq!((read[0].2, read[0].3), (0.2, true));
        assert_eq!((read[1].2, read[1].3), (0.1, false));
        assert!(prints(Venue::BybitSpot, r#"{"success":true}"#).is_empty());
    }

    #[test]
    fn a_trade_is_worth_price_times_size() {
        let frame: Value = serde_json::from_str(r#"{"e":"aggTrade","s":"BTCUSDT","p":"80000","q":"0.25","m":false}"#).unwrap();
        let mut into = Vec::new();
        read_prints(Venue::BinancePerp, &frame, &mut into);
        assert_eq!(into[0].usd(), 20_000.0);
    }

    #[test]
    fn okx_and_hyperliquid_are_read_in_their_own_names() {
        let okx = r#"{"arg":{"channel":"trades","instId":"BTC-USDT-SWAP"},"data":[{"instId":"BTC-USDT-SWAP","px":"84100.5","sz":"3","side":"buy","ts":"1790000000000"}]}"#;
        assert_eq!(prints(Venue::OkxPerp, okx), [("BTC-USDT-SWAP".to_string(), 84_100.5, 3.0, true)]);
        assert_eq!(prints(Venue::OkxPerp, &okx.replace("\"side\":\"buy\"", "\"side\":\"sell\""))[0].3, false);
        assert!(prints(Venue::OkxPerp, r#"{"event":"subscribe","arg":{"channel":"trades","instId":"BTC-USDT-SWAP"}}"#).is_empty());
        assert!(prints(Venue::OkxPerp, "pong").is_empty(), "the answer to a ping is not a trade");

        let hl = r#"{"channel":"trades","data":[{"coin":"BTC","side":"B","px":"84120.0","sz":"0.5","time":1790000000000},{"coin":"BTC","side":"A","px":"84119.0","sz":"0.1","time":1790000000001}]}"#;
        let read = prints(Venue::HyperliquidPerp, hl);
        assert_eq!(read.len(), 2);
        assert_eq!((read[0].0.as_str(), read[0].3), ("BTC", true), "B is the aggressor buying");
        assert_eq!(read[1].3, false);
        assert!(prints(Venue::HyperliquidPerp, r#"{"channel":"subscriptionResponse","data":{}}"#).is_empty());
    }

    #[test]
    fn each_exchange_is_asked_in_its_own_words() {
        let symbols = ["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        assert_eq!(subscribe(Venue::BinanceSpot, &symbols), [r#"{"method":"SUBSCRIBE","params":["btcusdt@aggTrade","ethusdt@aggTrade"],"id":1}"#]);
        assert_eq!(subscribe(Venue::BybitPerp, &symbols), [r#"{"op":"subscribe","args":["publicTrade.BTCUSDT","publicTrade.ETHUSDT"]}"#]);
        let okx = ["BTC-USDT-SWAP".to_string()];
        assert_eq!(subscribe(Venue::OkxPerp, &okx), [r#"{"op":"subscribe","args":[{"channel":"trades","instId":"BTC-USDT-SWAP"}]}"#]);
        let coins = ["BTC".to_string(), "kPEPE".to_string()];
        assert_eq!(
            subscribe(Venue::HyperliquidPerp, &coins),
            [r#"{"method":"subscribe","subscription":{"type":"trades","coin":"BTC"}}"#, r#"{"method":"subscribe","subscription":{"type":"trades","coin":"kPEPE"}}"#],
            "Hyperliquid takes one subscription at a time"
        );

        assert!(stream_urls(Venue::KrakenPerp).is_empty(), "a venue nothing follows has no trades to read");
        assert!(stream_urls(Venue::BinancePerp).len() > 1, "an address that goes quiet is retried on the next one");
        assert!(ping(Venue::BybitSpot).is_some() && ping(Venue::BinanceSpot).is_none());
        assert_eq!(ping(Venue::OkxPerp), Some("ping"), "OKX wants the bare word");
        assert!(renames(Venue::OkxPerp) && renames(Venue::HyperliquidPerp) && !renames(Venue::BinancePerp));
    }
}
