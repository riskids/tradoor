//! Keeps every tracked market's history current: once a day is closed its
//! minutes are stored, months and years roll up as they close, and the manifest
//! is rebuilt. A pass with nothing to do only stats files, so running it often
//! is cheap.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tradoor_ingest::{Config, Ingest};
use tradoor_store::{now_ms, DataDir, Db};
use tradoor_venues::binance;

#[derive(Parser)]
#[command(about = "Keep tracked markets' candle history up to date")]
struct Args {
    #[arg(long, env = "TRADOOR_DATA_DIR", default_value = "./data")]
    data_dir: PathBuf,
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,
    /// Seconds between passes.
    #[arg(long, env = "TRADOOR_RECORDER_INTERVAL", default_value_t = 600)]
    interval: u64,
    #[arg(long, env = "BINANCE_SPOT_REST", default_value = binance::SPOT_REST)]
    binance_spot_rest: String,
    /// Run one pass and exit.
    #[arg(long)]
    once: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let args = Args::parse();
    let mut db = Db::connect(&args.database_url).await.context("connecting to Postgres")?;
    db.migrate().await?;
    let ingest = Ingest::new(DataDir::new(&args.data_dir), Config { binance_spot_rest: args.binance_spot_rest.clone(), ..Config::default() })?;

    loop {
        for tracked in db.tracked_markets().await? {
            let id = db.start_run("recorder", Some(&tracked.market)).await?;
            let result = ingest.sync_market(&tracked.market, tracked.history_from, now_ms(), Some(&mut db)).await;
            let stats = result.as_ref().map(|s| serde_json::to_value(s).unwrap_or_default()).unwrap_or_default();
            // One market failing — an exchange down, a delisting — must not stop the rest.
            if let Err(e) = &result {
                tracing::error!(market = %tracked.market, error = %e, "sync failed");
            }
            db.finish_run(id, result.is_ok(), stats, result.err().map(|e| e.to_string())).await?;
        }
        if args.once {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(args.interval)) => {}
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("shutting down");
                return Ok(());
            }
        }
    }
}
