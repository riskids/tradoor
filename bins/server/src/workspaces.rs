//! The terminal layout, stored whole. Writes carry the version they were based
//! on, so two tabs editing one workspace get a 409 instead of silently
//! overwriting each other.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{ApiError, ApiResult, AppState};

const MAX_DOC_BYTES: usize = 512 * 1024;

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub async fn get(State(state): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if !valid_id(&id) {
        return Err(ApiError::bad_request("workspace ids are lowercase letters, digits and dashes"));
    }
    let client = state.pool.get().await?;
    let row = client.query_opt("SELECT name, doc, version FROM workspace WHERE id = $1", &[&id]).await?;
    let row = row.ok_or_else(|| ApiError::not_found(format!("workspace '{id}' not found")))?;
    Ok(Json(json!({ "id": id, "name": row.get::<_, String>(0), "doc": row.get::<_, Value>(1), "version": row.get::<_, i32>(2) })))
}

#[derive(Deserialize)]
pub struct PutBody {
    name: String,
    doc: Value,
    /// The version this write is based on; 0 creates.
    version: i32,
}

pub async fn put(State(state): State<AppState>, Path(id): Path<String>, Json(body): Json<PutBody>) -> ApiResult<(StatusCode, Json<Value>)> {
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
    let client = state.pool.get().await?;
    let updated = client
        .query_opt("UPDATE workspace SET name = $2, doc = $3, version = version + 1, updated_at = now() WHERE id = $1 AND version = $4 RETURNING version", &[&id, &name, &body.doc, &body.version])
        .await?;
    if let Some(row) = updated {
        return Ok((StatusCode::OK, Json(json!({ "version": row.get::<_, i32>(0) }))));
    }
    if body.version == 0 {
        let created = client.query_opt("INSERT INTO workspace (id, name, doc) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING RETURNING version", &[&id, &name, &body.doc]).await?;
        if let Some(row) = created {
            return Ok((StatusCode::CREATED, Json(json!({ "version": row.get::<_, i32>(0) }))));
        }
    }
    let current = client.query_opt("SELECT doc, version FROM workspace WHERE id = $1", &[&id]).await?;
    match current {
        Some(row) => Ok((StatusCode::CONFLICT, Json(json!({ "error": "the workspace changed since it was loaded", "doc": row.get::<_, Value>(0), "version": row.get::<_, i32>(1) })))),
        None => Err(ApiError::not_found(format!("workspace '{id}' not found; create it with version 0"))),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn ids() {
        assert!(super::valid_id("default"));
        assert!(super::valid_id("scalping-2"));
        assert!(!super::valid_id("../x"));
        assert!(!super::valid_id("Default"));
        assert!(!super::valid_id(""));
    }
}
