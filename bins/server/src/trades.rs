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
        _ => &[],
    }
}

pub fn subscribe(venue: Venue, symbols: &[String]) -> String {
    match venue {
        Venue::BybitSpot | Venue::BybitPerp => {
            let args: Vec<String> = symbols.iter().map(|s| format!("\"publicTrade.{s}\"")).collect();
            format!("{{\"op\":\"subscribe\",\"args\":[{}]}}", args.join(","))
        }
        _ => {
            let params: Vec<String> = symbols.iter().map(|s| format!("\"{}@aggTrade\"", s.to_lowercase())).collect();
            format!("{{\"method\":\"SUBSCRIBE\",\"params\":[{}],\"id\":1}}", params.join(","))
        }
    }
}

/// Bybit closes a socket that says nothing for 20 seconds; Binance does not ask.
pub fn ping(venue: Venue) -> Option<&'static str> {
    matches!(venue, Venue::BybitSpot | Venue::BybitPerp).then_some("{\"op\":\"ping\"}")
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
    fn each_exchange_is_asked_in_its_own_words() {
        let symbols = ["BTCUSDT".to_string(), "ETHUSDT".to_string()];
        assert_eq!(subscribe(Venue::BinanceSpot, &symbols), r#"{"method":"SUBSCRIBE","params":["btcusdt@aggTrade","ethusdt@aggTrade"],"id":1}"#);
        assert_eq!(subscribe(Venue::BybitPerp, &symbols), r#"{"op":"subscribe","args":["publicTrade.BTCUSDT","publicTrade.ETHUSDT"]}"#);
        assert!(stream_urls(Venue::OkxPerp).is_empty(), "a venue with no chart has no trades to follow");
        assert!(stream_urls(Venue::BinancePerp).len() > 1, "an address that goes quiet is retried on the next one");
        assert!(ping(Venue::BybitSpot).is_some() && ping(Venue::BinanceSpot).is_none());
    }
}
