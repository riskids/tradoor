//! tradoor-server — the small API next to the static files.
//!
//! It serves the market catalogue, drawings and workspaces, and (in development)
//! the candle partitions and the built web app. Market data never passes through
//! it: the browser streams from the exchanges directly and reads history as
//! static files, which nginx and the CDN serve in production.

mod api;
mod catalog;
mod drawings;
mod import;
mod workspaces;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tradoor_store::{DataDir, Db};
use tradoor_venues::binance;

#[derive(Parser)]
#[command(about = "tradoor API server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(long, env = "DATABASE_URL", global = true)]
    database_url: Option<String>,
    #[arg(long, env = "TRADOOR_DATA_DIR", default_value = "./data", global = true)]
    data_dir: PathBuf,
    #[arg(long, env = "TRADOOR_BIND", default_value = "127.0.0.1:8080")]
    bind: SocketAddr,
    /// Serve the built web app from here as well (development and single-box setups).
    #[arg(long, env = "TRADOOR_WEB_DIR")]
    web_dir: Option<PathBuf>,
    /// When set, writes (drawings, workspaces) require `Authorization: Bearer <token>`.
    #[arg(long, env = "TRADOOR_API_TOKEN")]
    api_token: Option<String>,
    #[arg(long, env = "BINANCE_SPOT_REST", default_value = binance::SPOT_REST)]
    binance_spot_rest: String,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (the default).
    Serve,
    /// Import drawings exported from rsalianto (JSON lines; see scripts/export-rsalianto-drawings.sh).
    ImportDrawings { file: PathBuf },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();
    let cli = Cli::parse();
    let database_url = cli.database_url.clone().context("DATABASE_URL is required")?;

    // Migrations run on a dedicated connection before the pool opens, so every
    // pooled connection starts on the current schema.
    let mut db = Db::connect(&database_url).await.context("connecting to Postgres")?;
    db.migrate().await?;

    let pool = deadpool_postgres::Config { url: Some(database_url), ..Default::default() }
        .create_pool(Some(deadpool_postgres::Runtime::Tokio1), tokio_postgres::NoTls)
        .context("creating the connection pool")?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::ImportDrawings { file } => import::run(&pool, &file).await,
        Command::Serve => {
            let state = api::AppState {
                pool,
                data: DataDir::new(&cli.data_dir),
                catalog: Arc::new(catalog::Catalog::new(cli.binance_spot_rest.clone())?),
                api_token: cli.api_token.clone().filter(|t| !t.is_empty()),
            };
            let app = api::router(state, cli.data_dir.clone(), cli.web_dir.clone());
            let listener = tokio::net::TcpListener::bind(cli.bind).await.with_context(|| format!("binding {}", cli.bind))?;
            tracing::info!(addr = %cli.bind, "listening");
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
            Ok(())
        }
    }
}
