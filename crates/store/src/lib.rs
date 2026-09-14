//! tradoor-store — where history is kept.
//!
//! Files are the truth: `data/{venue}/{SYMBOL}/{base}/{period}.tdc.gz`, plus a
//! `manifest.json` per market that the browser reads. Postgres indexes them and
//! holds the things that are not candles.
#![forbid(unsafe_code)]

mod data;
mod db;

pub use data::DataDir;
pub use db::{Db, TrackedMarket};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Postgres(#[from] tokio_postgres::Error),
    #[error(transparent)]
    Migration(#[from] refinery::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Codec(#[from] tradoor_codec::CodecError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}
