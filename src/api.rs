use crate::{analysis::Analyzer, archive::Archive, metrics::Metrics, storage::Storage};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc, time::Instant};
#[derive(Clone)]
pub struct GrowthBaseline {
    pub measured_at: Instant,
    pub bytes: u64,
}
#[derive(Clone)]
pub struct AppState {
    pub analysis: Analyzer,
    pub metrics: Arc<Metrics>,
    pub storage: Storage,
    pub archive_dir: PathBuf,
    pub growth_baseline: GrowthBaseline,
}
pub fn router(s: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/api/stats", get(stats))
        .route("/api/trending/urls", get(urls))
        .route("/api/trending/domains", get(domains))
        .route("/api/trending/fingerprints", get(fingerprints))
        .route("/api/trending/conversations", get(conversations))
        .route("/api/anomalies", get(anomalies))
        .route("/api/clusters", get(clusters))
        .route("/api/clusters/{id}", get(cluster))
        .route("/api/accounts/{did}", get(account))
        .route("/api/storage", get(storage))
        .with_state(s)
}
async fn index() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}
async fn health() -> &'static str {
    "ok"
}
async fn ready(State(s): State<AppState>) -> impl IntoResponse {
    match s.storage.ping().await {
        Ok(()) => (StatusCode::OK, "ready"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "ClickHouse unavailable"),
    }
}
async fn metrics(State(s): State<AppState>) -> String {
    s.metrics
        .archive_bytes
        .set(Archive::total_size(&s.archive_dir) as i64);
    if let Ok(bytes) = s.storage.bytes().await {
        s.metrics.clickhouse_bytes.set(bytes as i64);
    }
    s.metrics.render()
}
async fn stats(State(s): State<AppState>) -> Json<crate::analysis::Rates> {
    Json(s.analysis.rates())
}
async fn urls(State(s): State<AppState>) -> Json<Vec<(String, u64)>> {
    Json(s.analysis.top(false))
}
async fn domains(State(s): State<AppState>) -> Json<Vec<(String, u64)>> {
    Json(s.analysis.top(true))
}
async fn fingerprints(State(s): State<AppState>) -> Json<Vec<crate::analysis::TrendingItem>> {
    Json(s.analysis.trending_fingerprints())
}
async fn conversations(State(s): State<AppState>) -> Json<Vec<crate::analysis::TrendingItem>> {
    Json(s.analysis.growing_conversations())
}
async fn anomalies(State(s): State<AppState>) -> Json<crate::analysis::CategorizedAnomalies> {
    Json(s.analysis.categorized_anomalies())
}
async fn clusters(State(s): State<AppState>) -> Json<Vec<crate::model::Detection>> {
    Json(s.analysis.detections())
}
async fn cluster(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    s.analysis
        .cluster_detail(&id)
        .map(Json)
        .map(IntoResponse::into_response)
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}
async fn account(
    State(s): State<AppState>,
    Path(did): Path<String>,
) -> Json<crate::analysis::AccountSummary> {
    Json(s.analysis.account(&did))
}
#[derive(Serialize)]
struct StorageStats {
    raw_archive_bytes: u64,
    clickhouse_bytes: u64,
    derived_bytes: u64,
    bytes_ingested_per_hour: f64,
    bytes_ingested_per_day: f64,
    projected_monthly_bytes: f64,
    projected_yearly_bytes: f64,
}
async fn storage(State(s): State<AppState>) -> Json<StorageStats> {
    let raw = Archive::total_size(&s.archive_dir);
    let ch = s.storage.bytes().await.unwrap_or(0);
    let derived = s.storage.table_bytes(Some("detections")).await.unwrap_or(0);
    s.metrics.clickhouse_bytes.set(ch as i64);
    let elapsed_hours = s.growth_baseline.measured_at.elapsed().as_secs_f64() / 3600.0;
    let current = raw.saturating_add(ch);
    let hourly = if elapsed_hours > 0.0 {
        current.saturating_sub(s.growth_baseline.bytes) as f64 / elapsed_hours
    } else {
        0.0
    };
    Json(StorageStats {
        raw_archive_bytes: raw,
        clickhouse_bytes: ch,
        derived_bytes: derived,
        bytes_ingested_per_hour: hourly,
        bytes_ingested_per_day: hourly * 24.0,
        projected_monthly_bytes: hourly * 24.0 * 30.0,
        projected_yearly_bytes: hourly * 24.0 * 365.0,
    })
}
