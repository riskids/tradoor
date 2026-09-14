//! `tradoor-backfill binance-perp:BTCUSDT --from 2021-09-01 --track`
//!
//! Downloads every closed minute since `--from`, rolls it up into hours and
//! days, and writes the partitions and manifest. Safe to re-run: anything already
//! on disk is skipped.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use tradoor_ingest::{Config, Ingest};
use tradoor_store::{now_ms, DataDir, Db};
use tradoor_venues::{binance, MarketRef};

#[derive(Parser)]
#[command(about = "Download and store candle history")]
struct Args {
    /// Markets as venue:SYMBOL, e.g. binance-perp:BTCUSDT.
    #[arg(required = true)]
    markets: Vec<String>,
    /// First UTC day to keep, YYYY-MM-DD.
    #[arg(long)]
    from: String,
    #[arg(long, env = "TRADOOR_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,
    /// Index partitions and log runs in Postgres. Without it only files are written.
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,
    /// Also hand the markets to the recorder, which keeps them current.
    #[arg(long)]
    track: bool,
    /// Use REST only, skipping data.binance.vision.
    #[arg(long)]
    no_archives: bool,
    #[arg(long, env = "BINANCE_SPOT_REST", default_value = binance::SPOT_REST)]
    binance_spot_rest: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let args = Args::parse();
    let from = tradoor_core::time::parse_iso(&args.from).context("--from must be YYYY-MM-DD")?;
    let markets = args
        .markets
        .iter()
        .map(|m| MarketRef::parse(m).with_context(|| format!("not a market: {m} (expected venue:SYMBOL, e.g. binance-perp:BTCUSDT)")))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut db = match &args.database_url {
        Some(url) => {
            let mut db = Db::connect(url).await.context("connecting to Postgres")?;
            db.migrate().await?;
            Some(db)
        }
        None => None,
    };
    let config = Config { binance_spot_rest: args.binance_spot_rest.clone(), use_archives: !args.no_archives, ..Config::default() };
    let ingest = Ingest::new(DataDir::new(&args.data_dir), config)?;

    for market in &markets {
        if let (true, Some(db)) = (args.track, db.as_ref()) {
            db.track(market, from).await?;
        }
        let run = match db.as_ref() {
            Some(db) => Some(db.start_run("backfill", Some(market)).await?),
            None => None,
        };
        let started = Instant::now();
        let result = ingest.sync_market(market, from, now_ms(), db.as_mut()).await;
        if let (Some(db), Some(id)) = (db.as_ref(), run) {
            let stats = result.as_ref().map(|s| serde_json::to_value(s).unwrap_or_default()).unwrap_or_default();
            db.finish_run(id, result.is_ok(), stats, result.as_ref().err().map(ToString::to_string)).await?;
        }
        let stats = result.with_context(|| format!("backfilling {market}"))?;
        tracing::info!(%market, elapsed = ?started.elapsed(), ?stats, "backfill finished");
    }
    Ok(())
}
