//! Venues and market references.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Exchange {
    Binance,
    Bybit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Spot,
    Perp,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spot => "spot",
            Self::Perp => "perp",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Venue {
    BinanceSpot,
    BinancePerp,
    BybitSpot,
    BybitPerp,
}

impl Venue {
    pub const ALL: [Venue; 4] = [Self::BinanceSpot, Self::BinancePerp, Self::BybitSpot, Self::BybitPerp];

    pub fn id(self) -> &'static str {
        match self {
            Self::BinanceSpot => "binance-spot",
            Self::BinancePerp => "binance-perp",
            Self::BybitSpot => "bybit-spot",
            Self::BybitPerp => "bybit-perp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::BinanceSpot => "Binance Spot",
            Self::BinancePerp => "Binance Perp",
            Self::BybitSpot => "Bybit Spot",
            Self::BybitPerp => "Bybit Perp",
        }
    }

    pub fn exchange(self) -> Exchange {
        match self {
            Self::BinanceSpot | Self::BinancePerp => Exchange::Binance,
            Self::BybitSpot | Self::BybitPerp => Exchange::Bybit,
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::BinanceSpot | Self::BybitSpot => Kind::Spot,
            Self::BinancePerp | Self::BybitPerp => Kind::Perp,
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.id() == id)
    }
}

/// `binance-perp:BTCUSDT`. The venue is part of a market's identity: BTCUSDT
/// exists on all four venues at four prices.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MarketRef {
    pub venue: Venue,
    pub symbol: String,
}

fn valid_symbol(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

impl MarketRef {
    pub fn new(venue: Venue, symbol: &str) -> Option<Self> {
        valid_symbol(symbol).then(|| Self { venue, symbol: symbol.to_string() })
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (venue, symbol) = s.split_once(':')?;
        Self::new(Venue::parse(venue)?, symbol)
    }

    /// Where the market's files live under the data directory. Safe to join onto
    /// a path: the symbol is validated to `[A-Z0-9_]`.
    pub fn dir(&self) -> String {
        format!("{}/{}", self.venue.id(), self.symbol)
    }
}

impl fmt::Display for MarketRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.venue.id(), self.symbol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_parse_and_refuse_paths() {
        let m = MarketRef::parse("binance-perp:BTCUSDT").unwrap();
        assert_eq!((m.venue, m.symbol.as_str(), m.to_string(), m.dir()), (Venue::BinancePerp, "BTCUSDT", "binance-perp:BTCUSDT".to_string(), "binance-perp/BTCUSDT".to_string()));
        assert!(MarketRef::parse("binance-perp:../etc").is_none());
        assert!(MarketRef::parse("binance-perp:btcusdt").is_none());
        assert!(MarketRef::parse("kraken-spot:BTCUSD").is_none());
        assert!(MarketRef::parse("BTCUSDT").is_none());
    }
}
