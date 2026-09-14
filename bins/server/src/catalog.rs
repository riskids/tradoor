//! The market catalogue a symbol search offers: every tradable spot and
//! perpetual market on Binance and Bybit, with 24h stats.
//!
//! Fetched here rather than by each browser because the raw lists are several
//! megabytes; the browser gets one compact, compressed response it can search
//! locally without a request per keystroke.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use serde::Serialize;
use tokio::sync::Mutex;
use tradoor_venues::{binance, bybit, Exchange, Product, Venue};

const TTL: Duration = Duration::from_secs(30 * 60);

pub struct Catalog {
    http: reqwest::Client,
    binance_spot_rest: String,
    state: Mutex<Option<Cached>>,
}

struct Cached {
    at: Instant,
    body: Arc<Bytes>,
    by_venue: Vec<(Venue, Vec<Product>)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VenueStatus {
    id: &'static str,
    label: &'static str,
    count: usize,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response<'a> {
    updated_at: i64,
    venues: Vec<VenueStatus>,
    products: Vec<&'a Product>,
}

impl Catalog {
    pub fn new(binance_spot_rest: String) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder().user_agent(concat!("tradoor/", env!("CARGO_PKG_VERSION"))).timeout(Duration::from_secs(30)).build()?;
        Ok(Self { http, binance_spot_rest, state: Mutex::new(None) })
    }

    async fn text(&self, url: &str) -> Result<String, String> {
        let resp = self.http.get(url).send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        let body = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("{status}: {}", body.chars().take(200).collect::<String>()));
        }
        Ok(body)
    }

    async fn fetch(&self, venue: Venue) -> Result<Vec<Product>, String> {
        match venue.exchange() {
            Exchange::Binance => {
                let base = if venue == Venue::BinanceSpot { self.binance_spot_rest.as_str() } else { binance::PERP_REST };
                let (info_url, tickers_url) = (binance::exchange_info_url(base, venue), binance::tickers_url(base, venue));
                let (info, tickers) = tokio::join!(self.text(&info_url), self.text(&tickers_url));
                binance::parse_products(venue, &info?, tickers.ok().as_deref()).map_err(|e| e.to_string())
            }
            Exchange::Bybit => {
                let tickers = self.text(&bybit::tickers_url(bybit::REST, venue)).await.ok();
                let mut out = Vec::new();
                let mut cursor: Option<String> = None;
                for _ in 0..20 {
                    let page = self.text(&bybit::instruments_url(bybit::REST, venue, cursor.as_deref())).await?;
                    let (products, next) = bybit::parse_instruments(venue, &page, tickers.as_deref()).map_err(|e| e.to_string())?;
                    out.extend(products);
                    match next {
                        Some(c) => cursor = Some(c),
                        None => break,
                    }
                }
                Ok(out)
            }
        }
    }

    /// The serialised catalogue, refreshed when older than the TTL. A venue that
    /// fails keeps its previous list: stale names beat an empty search box.
    pub async fn get(&self) -> Arc<Bytes> {
        let mut state = self.state.lock().await;
        if let Some(cached) = state.as_ref().filter(|c| c.at.elapsed() < TTL) {
            return cached.body.clone();
        }
        let results = futures_join(Venue::ALL.iter().map(|v| self.fetch(*v)).collect()).await;
        let previous = state.take().map(|c| c.by_venue).unwrap_or_default();
        let mut by_venue = Vec::new();
        let mut venues = Vec::new();
        for (venue, result) in Venue::ALL.into_iter().zip(results) {
            let (products, error) = match result {
                Ok(p) => (p, None),
                Err(e) => {
                    tracing::warn!(venue = venue.id(), error = %e, "catalogue fetch failed");
                    (previous.iter().find(|(v, _)| *v == venue).map(|(_, p)| p.clone()).unwrap_or_default(), Some(e))
                }
            };
            venues.push(VenueStatus { id: venue.id(), label: venue.label(), count: products.len(), error });
            by_venue.push((venue, products));
        }
        let response = Response { updated_at: tradoor_store::now_ms(), venues, products: by_venue.iter().flat_map(|(_, p)| p).collect() };
        let body = Arc::new(Bytes::from(serde_json::to_vec(&response).expect("catalogue serialises")));
        *state = Some(Cached { at: Instant::now(), body: body.clone(), by_venue });
        body
    }
}

async fn futures_join<F: std::future::Future>(futures: Vec<F>) -> Vec<F::Output> {
    let handles: Vec<_> = futures.into_iter().collect();
    let mut out = Vec::with_capacity(handles.len());
    // Four requests; running them one after another inside one task is simpler
    // than spawning and costs a second or two once every half hour.
    for f in handles {
        out.push(f.await);
    }
    out
}
