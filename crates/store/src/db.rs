use tokio_postgres::{Client, NoTls};
use tradoor_codec::manifest::Manifest;
use tradoor_core::time::{iso_date, parse_iso};
use tradoor_venues::MarketRef;

use crate::StoreError;

mod embedded {
    refinery::embed_migrations!("../../migrations");
}

pub struct Db {
    client: Client,
}

#[derive(Debug, Clone)]
pub struct TrackedMarket {
    pub market: MarketRef,
    /// Epoch ms of the first day kept.
    pub history_from: i64,
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self, StoreError> {
        let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!(error = %e, "postgres connection closed");
            }
        });
        Ok(Self { client })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn migrate(&mut self) -> Result<(), StoreError> {
        let report = embedded::migrations::runner().run_async(&mut self.client).await?;
        for m in report.applied_migrations() {
            tracing::info!(migration = %m, "applied");
        }
        Ok(())
    }

    /// Replace the partition index for one market with what the manifest lists.
    pub async fn sync_partitions(&mut self, market: &MarketRef, manifest: &Manifest) -> Result<(), StoreError> {
        let tx = self.client.transaction().await?;
        let m = market.to_string();
        tx.execute("DELETE FROM partition WHERE market = $1", &[&m]).await?;
        let stmt = tx
            .prepare("INSERT INTO partition (market, base, period, rows, bytes, sha256, first_open, last_open, source) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'sync')")
            .await?;
        for (base, list) in &manifest.bases {
            for p in list {
                let (rows, bytes) = (p.rows as i32, p.bytes as i32);
                tx.execute(&stmt, &[&m, base, &p.period, &rows, &bytes, &p.sha256, &p.first_open, &p.last_open]).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn tracked_markets(&self) -> Result<Vec<TrackedMarket>, StoreError> {
        let rows = self.client.query("SELECT market, history_from::text FROM tracked_market WHERE enabled ORDER BY market", &[]).await?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                let market = MarketRef::parse(r.get::<_, &str>(0))?;
                Some(TrackedMarket { market, history_from: parse_iso(r.get::<_, &str>(1))? })
            })
            .collect())
    }

    /// Start keeping a market, or extend how far back it is kept.
    pub async fn track(&self, market: &MarketRef, from: i64) -> Result<(), StoreError> {
        self.client
            .execute(
                "INSERT INTO tracked_market (market, history_from) VALUES ($1, $2::text::date)
                 ON CONFLICT (market) DO UPDATE SET history_from = least(tracked_market.history_from, excluded.history_from), enabled = TRUE",
                &[&market.to_string(), &iso_date(from)],
            )
            .await?;
        Ok(())
    }

    pub async fn start_run(&self, job: &str, market: Option<&MarketRef>) -> Result<i64, StoreError> {
        let row = self.client.query_one("INSERT INTO ingest_run (job, market, status) VALUES ($1, $2, 'running') RETURNING id", &[&job, &market.map(ToString::to_string)]).await?;
        Ok(row.get(0))
    }

    pub async fn finish_run(&self, id: i64, ok: bool, stats: serde_json::Value, error: Option<String>) -> Result<(), StoreError> {
        self.client
            .execute(
                "UPDATE ingest_run SET status = $2, finished_at = now(), stats = $3, error = $4 WHERE id = $1",
                &[&id, &if ok { "success" } else { "failed" }, &stats, &error],
            )
            .await?;
        Ok(())
    }

    pub async fn record_gap(&self, market: &MarketRef, base: &str, from_open: i64, to_open: i64, reason: &str) -> Result<(), StoreError> {
        self.client
            .execute(
                "INSERT INTO data_gap (market, base, from_open, to_open, reason) VALUES ($1, $2, $3, $4, $5)
                 ON CONFLICT (market, base, from_open) DO UPDATE SET to_open = excluded.to_open, reason = excluded.reason",
                &[&market.to_string(), &base, &from_open, &to_open, &reason],
            )
            .await?;
        Ok(())
    }
}
