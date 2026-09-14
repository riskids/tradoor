//! `tradoor-server import-drawings drawings.jsonl`
//!
//! One JSON object per line: `{market, timeframe, kind, points, style, label}`,
//! as written by scripts/export-rsalianto-drawings.sh. Every line is validated
//! exactly as the API validates a new drawing; a line that fails is reported and
//! skipped, never half-imported.

use std::path::Path;

use anyhow::Context;
use serde_json::Value;
use tradoor_core::ChartTimeframe;
use tradoor_venues::MarketRef;

use crate::drawings::{insert, parse_body};

pub async fn run(pool: &deadpool_postgres::Pool, file: &Path) -> anyhow::Result<()> {
    let text = tokio::fs::read_to_string(file).await.with_context(|| format!("reading {}", file.display()))?;
    let client = pool.get().await?;
    let (mut imported, mut skipped) = (0, 0);
    for (n, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        let result = (|| -> Result<_, String> {
            let v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
            let market = v.get("market").and_then(Value::as_str).and_then(MarketRef::parse).ok_or("market must be venue:SYMBOL")?;
            let timeframe = v.get("timeframe").and_then(Value::as_str).filter(|t| ChartTimeframe::parse(t).is_some()).ok_or("invalid timeframe")?;
            Ok((market.to_string(), timeframe.to_string(), parse_body(&v)?))
        })();
        match result {
            Ok((market, timeframe, input)) => {
                insert(&client, &market, &timeframe, &input).await?;
                imported += 1;
            }
            Err(e) => {
                tracing::warn!(line = n + 1, error = %e, "skipped");
                skipped += 1;
            }
        }
    }
    tracing::info!(imported, skipped, "import finished");
    Ok(())
}
