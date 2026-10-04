//! tradoor-server — the small API next to the static files.
//!
//! It serves the market catalogue, drawings and workspaces, and (in development)
//! the candle partitions and the built web app. Market data does not normally
//! pass through it: the browser streams from the exchanges directly and reads
//! history as static files, which nginx and the CDN serve in production. The one
//! exception is the relay (`relay.rs`), which a browser falls back to only when
//! its network blocks the exchanges' names.

mod account;
mod alert_watch;
mod alerts;
mod api;
mod catalog;
mod dns;
mod drives;
mod drawings;
mod flow;
mod footprint;
mod import;
mod limits;
mod liquidations;
mod open_interest;
mod relay;
mod signals;
mod telegram;
mod trades;
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
    #[arg(long, env = "BINANCE_SPOT_REST", default_value = binance::SPOT_REST)]
    binance_spot_rest: String,
    /// Tickers whose liquidations are kept for 24 hours, gathered from every
    /// perpetual exchange. An empty list turns the collection off.
    #[arg(long, env = "TRADOOR_LIQUIDATIONS", default_value = "BTCUSDT,ETHUSDT,SOLUSDT", value_delimiter = ',')]
    liquidations: Vec<String>,
    /// Coins whose open interest is recorded from the start; a chart asking for
    /// another adds it. An empty list turns the recording off.
    #[arg(long, env = "TRADOOR_OPEN_INTEREST", default_value = "BTC,ETH,SOL", value_delimiter = ',')]
    open_interest: Vec<String>,
    /// A Telegram bot's token (from @BotFather), for alerts sent to a chat.
    /// Without it the Telegram section of Settings says it is not set up.
    #[arg(long, env = "TRADOOR_TELEGRAM_TOKEN", default_value = "")]
    telegram_token: String,
    /// Browser ids (Settings → Browser id) that may choose which coins' flow the
    /// server follows besides BTC. Without one, only BTC's is followed.
    #[arg(long, env = "TRADOOR_ADMINS", default_value = "", value_delimiter = ',')]
    admins: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (the default).
    Serve,
    /// Import drawings exported from rsalianto (JSON lines; see scripts/export-rsalianto-drawings.sh).
    ImportDrawings {
        file: PathBuf,
        /// The browser id they go to (Settings → Browser id); without it they are nobody's until adopted.
        #[arg(long, default_value = "")]
        owner: String,
    },
    /// Give the drawings and workspace that have no owner — everything from
    /// before browsers had ids — to one browser id (Settings → Browser id).
    Adopt { owner: String },
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
        Command::ImportDrawings { file, owner } => {
            if !owner.is_empty() && !api::valid_owner(&owner) {
                anyhow::bail!("--owner must be a browser id: 16 to 64 letters, digits and dashes");
            }
            import::run(&pool, &file, &owner).await
        }
        Command::Adopt { owner } => {
            if !api::valid_owner(&owner) {
                anyhow::bail!("a browser id is 16 to 64 letters, digits and dashes — copy it from Settings");
            }
            let client = pool.get().await?;
            let drawings = client.execute("UPDATE drawing SET owner = $1 WHERE owner = ''", &[&owner]).await?;
            // The browser's own workspace, if it saved one, stays; the old shared one is dropped then.
            let workspaces = client.execute("UPDATE workspace w SET owner = $1 WHERE owner = '' AND NOT EXISTS (SELECT 1 FROM workspace o WHERE o.owner = $1 AND o.id = w.id)", &[&owner]).await?;
            tracing::info!(drawings, workspaces, owner = api::short_owner(&owner), "adopted");
            Ok(())
        }
        Command::Serve => {
            let tickers: Vec<String> = cli.liquidations.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            if let Some(bad) = tickers.iter().find(|t| !liquidations::is_ticker(t)) {
                anyhow::bail!("TRADOOR_LIQUIDATIONS: {bad:?} is not a ticker — write them like BTCUSDT,ETHUSDT");
            }
            let admins: Vec<String> = cli.admins.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
            if admins.iter().any(|a| !api::valid_owner(a)) {
                anyhow::bail!("TRADOOR_ADMINS: a browser id is 16 to 64 letters, digits and dashes — copy it from Settings");
            }
            let doh = dns::Doh::new()?;
            let coins: Vec<String> = cli.open_interest.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
            if let Some(bad) = coins.iter().find(|c| open_interest::Coin::parse(c).is_none()) {
                anyhow::bail!("TRADOOR_OPEN_INTEREST: {bad:?} is not a coin — write them like BTC,ETH,1000PEPE");
            }
            let open_interest = if coins.is_empty() { None } else { Some(open_interest::Recorder::start(pool.clone(), coins, &doh).await?) };
            let token = cli.telegram_token.trim().to_string();
            let telegram = if token.is_empty() { None } else { Some(telegram::Telegram::start(pool.clone(), token, &doh)?) };
            let catalog = Arc::new(catalog::Catalog::new(cli.binance_spot_rest.clone(), &doh)?);
            alert_watch::start(pool.clone(), doh.clone(), catalog.clone(), telegram.clone());
            let flow = Some(flow::Flow::start(pool.clone(), doh.clone(), catalog.clone(), telegram.clone(), admins));
            footprint::Footprint::start(pool.clone(), doh.clone(), catalog.clone());
            signals::start(pool.clone(), &doh, catalog.clone(), telegram.clone());
            let state = api::AppState {
                open_interest,
                flow,
                telegram,
                liquidations: (!tickers.is_empty()).then(|| liquidations::Collector::start(pool.clone(), tickers, doh.clone())),
                pool,
                data: DataDir::new(&cli.data_dir),
                catalog,
                relay: relay::Relay::new(doh)?,
                limits: Arc::new(limits::Limits::default()),
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
