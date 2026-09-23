use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use serde_json::json;
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tradoor_store::DataDir;

use crate::{catalog::Catalog, drawings, liquidations, open_interest, relay, workspaces};

#[derive(Clone)]
pub struct AppState {
    pub pool: deadpool_postgres::Pool,
    pub data: DataDir,
    pub catalog: Arc<Catalog>,
    /// Absent when no ticker's liquidations are being kept.
    pub liquidations: Option<Arc<liquidations::Collector>>,
    /// Absent when no coin's open interest is being recorded.
    pub open_interest: Option<Arc<open_interest::Recorder>>,
    /// The exchanges' market data, for browsers on networks that block them.
    pub relay: Arc<relay::Relay>,
}

/// An error with the status it answers with. The message goes to the client,
/// so it names the field or the rule — never a connection string.
#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<tokio_postgres::Error> for ApiError {
    fn from(e: tokio_postgres::Error) -> Self {
        tracing::error!(error = %e, "database");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "database error".into())
    }
}

impl From<deadpool_postgres::PoolError> for ApiError {
    fn from(e: deadpool_postgres::PoolError) -> Self {
        tracing::error!(error = %e, "pool");
        Self(StatusCode::SERVICE_UNAVAILABLE, "database unavailable".into())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// The header a browser names itself in: a random id it made and keeps.
pub const OWNER_HEADER: &str = "x-tradoor-owner";

/// Whose drawings and workspace a request is about. Long and random enough to
/// be unguessable: knowing it is what makes them yours.
pub fn owner(headers: &axum::http::HeaderMap) -> ApiResult<String> {
    let id = headers.get(OWNER_HEADER).and_then(|v| v.to_str().ok()).unwrap_or_default();
    if valid_owner(id) {
        Ok(id.to_string())
    } else {
        Err(ApiError::bad_request(format!("{OWNER_HEADER} must be this browser's id: 16 to 64 letters, digits and dashes")))
    }
}

pub fn valid_owner(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

pub fn router(state: AppState, data_dir: PathBuf, web_dir: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route("/health", get(health))
        .route("/markets", get(markets))
        .route("/history", get(history))
        .route("/drawings", get(drawings::list).post(drawings::create).delete(drawings::clear))
        .route("/drawings/{id}", delete(drawings::delete_one).patch(drawings::update))
        .route("/workspaces/{id}", get(workspaces::get).put(workspaces::put))
        .route("/liquidations/{ticker}", get(liquidations::stats))
        .route("/liquidations/{ticker}/live", get(liquidations::live))
        .route("/open-interest", get(open_interest::candles))
        .route("/relay/rest", get(relay::rest))
        .route("/relay/ws", get(relay::ws))
        .layer(CompressionLayer::new())
        .with_state(state);

    let data = Router::new().fallback_service(ServeDir::new(data_dir)).layer(middleware::from_fn(cache_partitions));

    let mut app = Router::new().nest("/api", api).nest("/data", data);
    if let Some(web) = web_dir {
        app = app.fallback_service(ServeDir::new(&web).fallback(ServeFile::new(web.join("index.html"))));
    }
    app.layer(TraceLayer::new_for_http())
}

/// Partition files never change once written, so they are cached for a year;
/// a manifest changes whenever the recorder runs and is always revalidated.
async fn cache_partitions(req: Request, next: Next) -> Response {
    let immutable = req.uri().path().ends_with(".tdc.gz");
    let mut res = next.run(req).await;
    let value = if immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db = match state.pool.get().await {
        Ok(client) => client.query_one("SELECT 1", &[]).await.is_ok(),
        Err(_) => false,
    };
    let status = if db { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(json!({ "ok": db, "db": db })))
}

async fn markets(State(state): State<AppState>) -> Response {
    let body = state.catalog.get().await;
    ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "public, max-age=300")], body.as_ref().clone()).into_response()
}

/// What history is on disk, per market and base — read from the manifests the
/// browser also reads, so the two can never disagree.
async fn history(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let data = state.data.clone();
    let markets = tokio::task::spawn_blocking(move || -> Result<Vec<serde_json::Value>, String> {
        let mut out = Vec::new();
        for market in data.markets().map_err(|e| e.to_string())? {
            let Ok(bytes) = std::fs::read(data.manifest_path(&market)) else { continue };
            let Ok(manifest) = serde_json::from_slice::<tradoor_codec::manifest::Manifest>(&bytes) else { continue };
            let bases: serde_json::Map<String, serde_json::Value> = manifest
                .bases
                .iter()
                .map(|(base, list)| {
                    let rows: u64 = list.iter().map(|p| u64::from(p.rows)).sum();
                    let bytes: u64 = list.iter().map(|p| p.bytes).sum();
                    (base.clone(), json!({ "from": list.first().map(|p| p.first_open), "to": list.last().map(|p| p.last_open), "rows": rows, "bytes": bytes, "files": list.len() }))
                })
                .collect();
            out.push(json!({ "market": manifest.market, "updatedAt": manifest.updated_at, "bases": bases }));
        }
        Ok(out)
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "markets": markets })))
}
