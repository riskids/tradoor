//! The terminal layout, stored whole, one set per browser (the owner the
//! request names). Writes carry the version they were based on, so two tabs
//! editing one workspace get a 409 instead of silently overwriting each other.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{owner, ApiError, ApiResult, AppState};

const MAX_DOC_BYTES: usize = 512 * 1024;
/// Named layouts one browser keeps.
const MAX_PER_OWNER: i64 = 50;
const MAX_NAME: usize = 64;
const MAX_SUMMARY: usize = 64;

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub async fn get(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    if !valid_id(&id) {
        return Err(ApiError::bad_request("workspace ids are lowercase letters, digits and dashes"));
    }
    let client = state.pool.get().await?;
    let row = client.query_opt("SELECT name, doc, version FROM workspace WHERE owner = $1 AND id = $2", &[&owner, &id]).await?;
    let row = row.ok_or_else(|| ApiError::not_found(format!("workspace '{id}' not found")))?;
    Ok(Json(json!({ "id": id, "name": row.get::<_, String>(0), "doc": row.get::<_, Value>(1), "version": row.get::<_, i32>(2) })))
}

#[derive(Deserialize)]
pub struct PutBody {
    name: String,
    doc: Value,
    /// The version this write is based on; 0 creates.
    version: i32,
    /// A one-line digest for the layout list, e.g. "BTCUSDT.P, 240".
    #[serde(default)]
    summary: Option<String>,
}

pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let owner = owner(&headers)?;
    let client = state.pool.get().await?;
    let rows = client
        .query(
            "SELECT id, name, summary, starred, (extract(epoch FROM updated_at) * 1000)::int8, (extract(epoch FROM opened_at) * 1000)::int8 FROM workspace WHERE owner = $1 ORDER BY opened_at DESC NULLS LAST, updated_at DESC",
            &[&owner],
        )
        .await?;
    let list: Vec<Value> = rows
        .iter()
        .map(|r| json!({ "id": r.get::<_, String>(0), "name": r.get::<_, String>(1), "summary": r.get::<_, String>(2), "starred": r.get::<_, bool>(3), "updatedAt": r.get::<_, i64>(4), "openedAt": r.get::<_, Option<i64>>(5) }))
        .collect();
    Ok(Json(Value::Array(list)))
}

#[derive(Deserialize)]
pub struct PatchBody {
    name: Option<String>,
    starred: Option<bool>,
    opened: Option<bool>,
}

/// Name, star and last-opened only: no version check, the document is not touched.
pub async fn patch(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(body): Json<PatchBody>) -> ApiResult<StatusCode> {
    let owner = owner(&headers)?;
    if !valid_id(&id) {
        return Err(ApiError::bad_request("workspace ids are lowercase letters, digits and dashes"));
    }
    let name = match body.name.as_deref().map(str::trim) {
        Some(n) if n.is_empty() || n.chars().count() > MAX_NAME => return Err(ApiError::bad_request(format!("name must be 1–{MAX_NAME} characters"))),
        other => other.map(str::to_string),
    };
    let opened = body.opened == Some(true);
    if name.is_none() && body.starred.is_none() && !opened {
        return Err(ApiError::bad_request("nothing to change: send name, starred or opened"));
    }
    let client = state.pool.get().await?;
    let n = client
        .execute(
            "UPDATE workspace SET name = COALESCE($3, name), starred = COALESCE($4, starred), opened_at = CASE WHEN $5 THEN now() ELSE opened_at END WHERE owner = $1 AND id = $2",
            &[&owner, &id, &name, &body.starred, &opened],
        )
        .await?;
    if n == 0 {
        return Err(ApiError::not_found(format!("workspace '{id}' not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The layout and the drawings that belong to it.
pub async fn delete(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let owner = owner(&headers)?;
    if !valid_id(&id) {
        return Err(ApiError::bad_request("workspace ids are lowercase letters, digits and dashes"));
    }
    let mut client = state.pool.get().await?;
    let tx = client.transaction().await?;
    let n = tx.execute("DELETE FROM workspace WHERE owner = $1 AND id = $2", &[&owner, &id]).await?;
    if n == 0 {
        return Err(ApiError::not_found(format!("workspace '{id}' not found")));
    }
    tx.execute("DELETE FROM drawing WHERE owner = $1 AND layout = $2", &[&owner, &id]).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn put(State(state): State<AppState>, headers: HeaderMap, Path(id): Path<String>, Json(body): Json<PutBody>) -> ApiResult<(StatusCode, Json<Value>)> {
    let owner = owner(&headers)?;
    if !valid_id(&id) {
        return Err(ApiError::bad_request("workspace ids are lowercase letters, digits and dashes"));
    }
    if !body.doc.is_object() {
        return Err(ApiError::bad_request("doc must be a JSON object"));
    }
    if serde_json::to_vec(&body.doc).map_or(0, |v| v.len()) > MAX_DOC_BYTES {
        return Err(ApiError::bad_request("workspace document is too large"));
    }
    let name = body.name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err(ApiError::bad_request("name must be 1–120 characters"));
    }
    let summary = body.summary.as_deref().map(str::trim);
    if summary.is_some_and(|s| s.chars().count() > MAX_SUMMARY) {
        return Err(ApiError::bad_request(format!("summary may be at most {MAX_SUMMARY} characters")));
    }
    let client = state.pool.get().await?;
    let updated = client
        .query_opt("UPDATE workspace SET name = $2, doc = $3, summary = COALESCE($6, summary), version = version + 1, updated_at = now() WHERE id = $1 AND version = $4 AND owner = $5 RETURNING version", &[&id, &name, &body.doc, &body.version, &owner, &summary])
        .await?;
    if let Some(row) = updated {
        return Ok((StatusCode::OK, Json(json!({ "version": row.get::<_, i32>(0) }))));
    }
    if body.version == 0 {
        let held: i64 = client.query_one("SELECT count(*) FROM workspace WHERE owner = $1", &[&owner]).await?.get(0);
        if held >= MAX_PER_OWNER {
            return Err(ApiError::bad_request(format!("a browser may keep at most {MAX_PER_OWNER} workspaces")));
        }
        if held == 0 && !crate::limits::new_owner(&state, &headers) {
            return Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many new browser ids from this address; wait a while".into()));
        }
        let created = client.query_opt("INSERT INTO workspace (owner, id, name, doc, summary) VALUES ($4, $1, $2, $3, COALESCE($5, '')) ON CONFLICT (owner, id) DO NOTHING RETURNING version", &[&id, &name, &body.doc, &owner, &summary]).await?;
        if let Some(row) = created {
            return Ok((StatusCode::CREATED, Json(json!({ "version": row.get::<_, i32>(0) }))));
        }
    }
    let current = client.query_opt("SELECT doc, version FROM workspace WHERE owner = $1 AND id = $2", &[&owner, &id]).await?;
    match current {
        Some(row) => Ok((StatusCode::CONFLICT, Json(json!({ "error": "the workspace changed since it was loaded", "doc": row.get::<_, Value>(0), "version": row.get::<_, i32>(1) })))),
        None => Err(ApiError::not_found(format!("workspace '{id}' not found; create it with version 0"))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn owners_are_long_random_ids() {
        assert!(crate::api::valid_owner("3f2a9c1e-7b4d-4e8a-9f1c-2d6b8a0e5c7f"));
        assert!(!crate::api::valid_owner("short"));
        assert!(!crate::api::valid_owner("has spaces in it here!"));
        assert!(!crate::api::valid_owner(&"a".repeat(65)));
    }

    #[test]
    fn ids() {
        assert!(super::valid_id("default"));
        assert!(super::valid_id("scalping-2"));
        assert!(!super::valid_id("../x"));
        assert!(!super::valid_id("Default"));
        assert!(!super::valid_id(""));
    }
}
