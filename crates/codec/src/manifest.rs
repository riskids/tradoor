//! `manifest.json`, one per market: which partition files exist. It is what the
//! browser reads before fetching any history.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub market: String,
    pub updated_at: i64,
    /// Keyed by base interval (`1m`, `1h`, `1d`), each list ordered by time.
    pub bases: BTreeMap<String, Vec<PartitionInfo>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartitionInfo {
    /// `2026`, `2026-08` or `2026-08-13`.
    pub period: String,
    pub rows: u32,
    pub bytes: u64,
    pub first_open: i64,
    pub last_open: i64,
    pub sha256: String,
}
