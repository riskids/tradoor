//! Price alerts, stored per browser id as drawings are. Two watchers report a
//! trigger here: the page, which holds the exchanges' streams already and so
//! fires the moment a chart sees it, and the server's own watcher in
//! `alert_watch`, which fires when no browser is open. The alert's state and
//! the log of what fired live here, so both follow the reader to any tab and
//! survive a reload.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tradoor_store::now_ms;
use tradoor_venues::MarketRef;

use crate::api::{owner, ApiError, ApiResult, AppState};

pub const CONDITIONS: [&str; 5] = ["crossing", "crossing_up", "crossing_down", "greater", "less"];
/// Alerts one browser keeps; TradingView's largest plan allows 800.
pub const MAX_PER_OWNER: i64 = 800;
/// Log entries kept per browser, newest first.
const LOG_KEPT: i64 = 500;
const MAX_MESSAGE: usize = 280;

const COLUMNS: &str = "id, market, condition, price, message, active, created_at, triggered_at, trigger_price";

fn row_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>(0), "market": r.get::<_, String>(1), "condition": r.get::<_, String>(2), "price": r.get::<_, f64>(3),
        "message": r.get::<_, Option<String>>(4), "active": r.get::<_, bool>(5), "createdAt": r.get::<_, i64>(6),
        "triggeredAt": r.get::<_, Option<i64>>(7), "triggerPrice": r.get::<_, Option<f64>>(8),
    })
}

fn log_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "id": r.get::<_, i64>(0), "alertId": r.get::<_, Option<i64>>(1), "market": r.get::<_, String>(2), "condition": r.get::<_, String>(3),
        "price": r.get::<_, f64>(4), "message": r.get::<_, Option<String>>(5), "time": r.get::<_, i64>(6), "value": r.get::<_, f64>(7),
    })
}

#[derive(Debug, PartialEq)]
pub struct AlertInput {
    pub market: String,
    pub condition: String,
    pub price: f64,
    pub message: Option<String>,
}

fn price(v: Option<&Value>) -> Result<f64, String> {
    v.and_then(Value::as_f64).filter(|p| p.is_finite() && *p > 0.0).ok_or_else(|| "price must be a positive number".to_string())
}

fn condition(v: Option<&Value>) -> Result<String, String> {
    v.and_then(Value::as_str).filter(|c| CONDITIONS.contains(c)).map(str::to_string).ok_or_else(|| format!("condition must be one of {}", CONDITIONS.join(", ")))
}

fn message(v: Option<&Value>) -> Result<Option<String>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.trim().is_empty() => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= MAX_MESSAGE => Ok(Some(s.trim().to_string())),
        _ => Err(format!("message must be text of at most {MAX_MESSAGE} characters")),
    }
}

/// An alert off the wire; the error names the field and becomes the 400.
pub fn parse_body(body: &Value) -> Result<AlertInput, String> {
    let b = body.as_object().ok_or("body must be a JSON object")?;
    let market = b.get("market").and_then(Value::as_str).and_then(MarketRef::parse).ok_or("market must be venue:SYMBOL")?.to_string();
    Ok(AlertInput { market, condition: condition(b.get("condition"))?, price: price(b.get("price"))?, message: message(b.get("message"))? })
}

pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let alerts = client.query(&format!("SELECT {COLUMNS} FROM alert WHERE owner = $1 ORDER BY id DESC"), &[&owner]).await?;
    let log = client
        .query("SELECT id, alert_id, market, condition, price, message, time, value FROM alert_log WHERE owner = $1 ORDER BY time DESC, id DESC LIMIT $2", &[&owner, &LOG_KEPT])
        .await?;
    Ok(Json(json!({ "alerts": alerts.iter().map(row_json).collect::<Vec<_>>(), "log": log.iter().map(log_json).collect::<Vec<_>>() })))
}

pub async fn create(State(state): State<AppState>, headers: HeaderMap, Json(body): Json<Value>) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner = owner(&headers)?;
    let input = parse_body(&body).map_err(ApiError::bad_request)?;
    let client = state.pool.get().await?;
    let held: i64 = client.query_one("SELECT count(*) FROM alert WHERE owner = $1", &[&owner]).await?.get(0);
    if held >= MAX_PER_OWNER {
        return Err(ApiError::bad_request(format!("a browser may keep at most {MAX_PER_OWNER} alerts; delete some first")));
    }
    let row = client
        .query_one(
            &format!("INSERT INTO alert (owner, market, condition, price, message, created_at) VALUES ($1, $2, $3, $4, $5, $6) RETURNING {COLUMNS}"),
            &[&owner, &input.market, &input.condition, &input.price, &input.message, &now_ms()],
        )
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "alert": row_json(&row) }))))
}

/// Stop an alert at the price that met it and write its log line, returning
/// both rows. `None` when it had already stopped: only an active alert fires,
/// so a second watcher reporting the same cross is not a second trigger.
pub async fn record_trigger(
    client: &tokio_postgres::Client,
    owner: &str,
    id: i64,
    value: f64,
    now: i64,
) -> Result<Option<(tokio_postgres::Row, tokio_postgres::Row)>, tokio_postgres::Error> {
    let fired = client
        .query_opt(&format!("UPDATE alert SET active = FALSE, triggered_at = $3, trigger_price = $4 WHERE id = $1 AND owner = $2 AND active RETURNING {COLUMNS}"), &[&id, &owner, &now, &value])
        .await?;
    let Some(fired) = fired else { return Ok(None) };
    let log = client
        .query_one(
            "INSERT INTO alert_log (owner, alert_id, market, condition, price, message, time, value) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             RETURNING id, alert_id, market, condition, price, message, time, value",
            &[&owner, &id, &fired.get::<_, String>(1), &fired.get::<_, String>(2), &fired.get::<_, f64>(3), &fired.get::<_, Option<String>>(4), &now, &value],
        )
        .await?;
    client
        .execute("DELETE FROM alert_log WHERE owner = $1 AND id NOT IN (SELECT id FROM alert_log WHERE owner = $1 ORDER BY time DESC, id DESC LIMIT $2)", &[&owner, &LOG_KEPT])
        .await?;
    Ok(Some((fired, log)))
}

/// Edit, restart or trigger one alert:
/// `{price?, condition?, message?}` edits it (and restarts it, as TradingView does);
/// `{active: true}` restarts a stopped one; `{triggered: {value}}` records a trigger
/// — the alert stops and the log gains a line.
pub async fn update(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<i64>, Json(body): Json<Value>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let b = body.as_object().ok_or_else(|| ApiError::bad_request("body must be a JSON object"))?;
    let client = state.pool.get().await?;
    let row = client.query_opt(&format!("SELECT {COLUMNS} FROM alert WHERE id = $1 AND owner = $2"), &[&id, &owner]).await?;
    let row = row.ok_or_else(|| ApiError::not_found(format!("alert {id} not found")))?;

    if let Some(t) = b.get("triggered") {
        let value = t.get("value").and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| ApiError::bad_request("triggered.value must be the price that fired it"))?;
        let now = now_ms();
        let Some((fired, log)) = record_trigger(&client, &owner, id, value, now).await? else {
            return Ok(Json(json!({ "alert": row_json(&row), "log": null })));
        };
        crate::telegram::notify_fired(state.telegram.as_ref(), &owner, &fired, value, now).await;
        return Ok(Json(json!({ "alert": row_json(&fired), "log": log_json(&log) })));
    }

    let new_price = match b.get("price") {
        Some(v) => price(Some(v)).map_err(ApiError::bad_request)?,
        None => row.get(3),
    };
    let new_condition = match b.get("condition") {
        Some(v) => condition(Some(v)).map_err(ApiError::bad_request)?,
        None => row.get(2),
    };
    let new_message = match b.get("message") {
        Some(v) => message(Some(v)).map_err(ApiError::bad_request)?,
        None => row.get(4),
    };
    let restart = b.get("active") == Some(&Value::Bool(true)) || b.contains_key("price") || b.contains_key("condition");
    let stop = b.get("active") == Some(&Value::Bool(false));
    let active = if stop { false } else { restart || row.get::<_, bool>(5) };
    let updated = client
        .query_one(
            &format!(
                "UPDATE alert SET price = $3, condition = $4, message = $5, active = $6,
                        triggered_at = CASE WHEN $7 THEN NULL ELSE triggered_at END,
                        trigger_price = CASE WHEN $7 THEN NULL ELSE trigger_price END
                 WHERE id = $1 AND owner = $2 RETURNING {COLUMNS}"
            ),
            &[&id, &owner, &new_price, &new_condition, &new_message, &active, &restart],
        )
        .await?;
    Ok(Json(json!({ "alert": row_json(&updated) })))
}

pub async fn delete_one(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let n = client.execute("DELETE FROM alert WHERE id = $1 AND owner = $2", &[&id, &owner]).await?;
    if n == 0 {
        return Err(ApiError::not_found(format!("alert {id} not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Which {
    /// `stopped` (the default), `active` or `all`.
    which: Option<String>,
}

/// Deletes many at once: every stopped alert, every active one, or all.
pub async fn clear(State(state): State<AppState>, headers: HeaderMap, Query(q): Query<Which>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let filter = match q.which.as_deref().unwrap_or("stopped") {
        "stopped" => "AND NOT active",
        "active" => "AND active",
        "all" => "",
        other => return Err(ApiError::bad_request(format!("which is stopped, active or all, not '{other}'"))),
    };
    let client = state.pool.get().await?;
    let n = client.execute(&format!("DELETE FROM alert WHERE owner = $1 {filter}"), &[&owner]).await?;
    Ok(Json(json!({ "deleted": n })))
}

/// Restarts every stopped alert.
pub async fn restart_all(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let n = client.execute("UPDATE alert SET active = TRUE, triggered_at = NULL, trigger_price = NULL WHERE owner = $1 AND NOT active", &[&owner]).await?;
    Ok(Json(json!({ "restarted": n })))
}

pub async fn clear_log(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<StatusCode> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    client.execute("DELETE FROM alert_log WHERE owner = $1", &[&owner]).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alert_needs_a_market_a_condition_and_a_price() {
        let ok = parse_body(&json!({ "market": "binance-perp:BTCUSDT", "condition": "crossing", "price": 75547.8, "message": "  " })).unwrap();
        assert_eq!(ok, AlertInput { market: "binance-perp:BTCUSDT".into(), condition: "crossing".into(), price: 75547.8, message: None });
        assert!(parse_body(&json!({ "market": "nowhere", "condition": "crossing", "price": 1 })).unwrap_err().contains("market"));
        assert!(parse_body(&json!({ "market": "binance-perp:BTCUSDT", "condition": "above", "price": 1 })).unwrap_err().contains("condition"));
        assert!(parse_body(&json!({ "market": "binance-perp:BTCUSDT", "condition": "less", "price": -1 })).unwrap_err().contains("price"));
        assert!(parse_body(&json!({ "market": "binance-perp:BTCUSDT", "condition": "less", "price": 1, "message": "x".repeat(300) })).unwrap_err().contains("message"));
    }
}
