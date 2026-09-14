use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tradoor_codec::manifest::{Manifest, PartitionInfo};
use tradoor_codec::partition::gunzip;
use tradoor_codec::{summarize, Period, TickCandles};
use tradoor_core::Interval;
use tradoor_venues::MarketRef;

use crate::{now_ms, sha256_hex, StoreError};

/// The bases history is kept at. Every chart timeframe folds from one of them.
pub const BASES: [Interval; 3] = [Interval::M1, Interval::H1, Interval::D1];

#[derive(Clone, Debug)]
pub struct DataDir {
    root: PathBuf,
}

impl DataDir {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn market_dir(&self, market: &MarketRef) -> PathBuf {
        self.root.join(market.venue.id()).join(&market.symbol)
    }

    pub fn path(&self, market: &MarketRef, base: Interval, period: Period) -> PathBuf {
        self.market_dir(market).join(base.as_str()).join(format!("{}.tdc.gz", period.name()))
    }

    pub fn manifest_path(&self, market: &MarketRef) -> PathBuf {
        self.market_dir(market).join("manifest.json")
    }

    pub fn exists(&self, market: &MarketRef, base: Interval, period: Period) -> bool {
        self.path(market, base, period).is_file()
    }

    pub fn modified(&self, market: &MarketRef, base: Interval, period: Period) -> Option<SystemTime> {
        fs::metadata(self.path(market, base, period)).ok()?.modified().ok()
    }

    /// Written to a temporary name and renamed into place, so a reader never sees
    /// half a file and a crash never leaves one behind under the real name.
    pub fn write(&self, market: &MarketRef, base: Interval, period: Period, ticks: &TickCandles) -> Result<(), StoreError> {
        let path = self.path(market, base, period);
        fs::create_dir_all(path.parent().expect("partition paths have a parent"))?;
        let tmp = path.with_extension("gz.tmp");
        fs::write(&tmp, ticks.encode_gz()?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn read(&self, market: &MarketRef, base: Interval, period: Period) -> Result<Option<TickCandles>, StoreError> {
        match fs::read(self.path(market, base, period)) {
            Ok(bytes) => Ok(Some(TickCandles::decode_gz(&bytes)?)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn remove(&self, market: &MarketRef, base: Interval, period: Period) -> Result<(), StoreError> {
        match fs::remove_file(self.path(market, base, period)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Periods on disk for one base, oldest first.
    pub fn periods(&self, market: &MarketRef, base: Interval) -> Result<Vec<Period>, StoreError> {
        let dir = self.market_dir(market).join(base.as_str());
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out: Vec<Period> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".tdc.gz").and_then(Period::parse))
            .collect();
        out.sort_by_key(|p| (p.start(), p.tier()));
        Ok(out)
    }

    /// Every market with a directory under the data root.
    pub fn markets(&self) -> Result<Vec<MarketRef>, StoreError> {
        let mut out = Vec::new();
        for venue in fs::read_dir(&self.root).into_iter().flatten().filter_map(Result::ok) {
            for symbol in fs::read_dir(venue.path()).into_iter().flatten().filter_map(Result::ok) {
                let (Some(v), Some(s)) = (venue.file_name().to_str().map(str::to_string), symbol.file_name().to_str().map(str::to_string)) else { continue };
                if let Some(m) = MarketRef::parse(&format!("{v}:{s}")) {
                    out.push(m);
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Rebuild `manifest.json` from what is actually on disk.
    pub fn write_manifest(&self, market: &MarketRef) -> Result<Manifest, StoreError> {
        let mut bases = BTreeMap::new();
        for base in BASES {
            let mut list = Vec::new();
            for period in self.periods(market, base)? {
                let bytes = fs::read(self.path(market, base, period))?;
                let summary = summarize(&gunzip(&bytes)?)?;
                list.push(PartitionInfo {
                    period: period.name(),
                    rows: summary.rows,
                    bytes: bytes.len() as u64,
                    first_open: summary.first_open,
                    last_open: summary.last_open,
                    sha256: sha256_hex(&bytes),
                });
            }
            if !list.is_empty() {
                bases.insert(base.as_str().to_string(), list);
            }
        }
        let manifest = Manifest { market: market.to_string(), updated_at: now_ms(), bases };
        let path = self.manifest_path(market);
        fs::create_dir_all(path.parent().expect("manifest has a parent"))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(&manifest)?)?;
        fs::rename(&tmp, &path)?;
        Ok(manifest)
    }
}
