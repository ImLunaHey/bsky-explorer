use crate::{
    analysis::Analyzer,
    archive::Archive,
    identity::{IdentityRecord, IdentityService},
    metrics::Metrics,
    storage::Storage,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, Request, StatusCode, header},
    middleware,
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, fs, path::PathBuf, sync::Arc, time::Instant};
use tokio::sync::RwLock;
use tower_http::services::{ServeDir, ServeFile};
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
    pub web_dist_dir: PathBuf,
    pub growth_baseline: GrowthBaseline,
    pub system_history: SystemHistory,
    pub identities: IdentityService,
}
pub type SystemHistory = Arc<RwLock<VecDeque<SystemSample>>>;
pub fn router(s: AppState) -> Router {
    let index = s.web_dist_dir.join("index.html");
    let frontend = ServeDir::new(&s.web_dist_dir).not_found_service(ServeFile::new(index));
    Router::new()
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
        .route("/api/identities", get(identities))
        .route("/api/storage", get(storage))
        .route("/api/system", get(system))
        .route("/api/system/history", get(system_history))
        .fallback_service(frontend)
        .layer(middleware::from_fn(cache_headers))
        .with_state(s)
}

async fn cache_headers(request: Request<axum::body::Body>, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let mut response = next.run(request).await;
    let value = cache_policy(&path);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    response
}

fn cache_policy(path: &str) -> &'static str {
    if path.starts_with("/api/") || matches!(path, "/metrics" | "/healthz" | "/readyz") {
        "no-store"
    } else if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        // Keep the SPA entry point fresh so a deployment never refers to evicted hashed assets.
        "no-cache"
    }
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

#[derive(Deserialize)]
struct IdentityQuery {
    dids: String,
}

async fn identities(
    State(s): State<AppState>,
    Query(query): Query<IdentityQuery>,
) -> Result<Json<Vec<IdentityRecord>>, (StatusCode, String)> {
    let dids: Vec<_> = query
        .dids
        .split(',')
        .map(str::trim)
        .filter(|did| !did.is_empty())
        .collect();
    if dids.is_empty() || dids.len() > 100 {
        return Err((
            StatusCode::BAD_REQUEST,
            "provide between 1 and 100 DIDs".into(),
        ));
    }
    let mut resolved = Vec::with_capacity(dids.len());
    for did in dids {
        resolved.push(
            s.identities
                .resolve(did)
                .await
                .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?,
        );
    }
    Ok(Json(resolved))
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

#[derive(Serialize)]
struct FilesystemStats {
    path: String,
    total_bytes: u64,
    available_bytes: u64,
    used_bytes: u64,
}

#[derive(Serialize)]
struct RuntimeStats {
    process_uptime_seconds: u64,
    process_resident_memory_bytes: u64,
    container_memory_used_bytes: Option<u64>,
    container_memory_limit_bytes: Option<u64>,
    process_cpu_seconds: Option<f64>,
    container_cpu_seconds: Option<f64>,
    host_cpu_total_jiffies: Option<u64>,
    host_cpu_idle_jiffies: Option<u64>,
    host_load_average: Option<[f64; 3]>,
    host_uptime_seconds: Option<f64>,
}

#[derive(Serialize)]
struct SystemStats {
    scope_note: &'static str,
    runtime: RuntimeStats,
    storage_array: FilesystemStats,
    clickhouse: Option<crate::storage::ClickHouseStats>,
    clickhouse_error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct SystemSample {
    timestamp: i64,
    events_per_second: f64,
    posts_per_second: f64,
    interactions_per_second: f64,
    process_resident_memory_bytes: u64,
    container_memory_used_bytes: Option<u64>,
    clickhouse_rows: Option<u64>,
    clickhouse_compressed_bytes: Option<u64>,
}

async fn system_history(State(s): State<AppState>) -> Json<Vec<SystemSample>> {
    Json(s.system_history.read().await.iter().cloned().collect())
}

pub async fn sample_system_history(state: AppState) {
    const MAX_SAMPLES: usize = 5_760; // 24 hours at the default 15-second interval.
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
    loop {
        interval.tick().await;
        let rates = state.analysis.rates();
        let clickhouse = state.storage.stats().await.ok();
        let sample = SystemSample {
            timestamp: chrono::Utc::now().timestamp_millis(),
            events_per_second: rates.events_per_sec,
            posts_per_second: rates.posts_per_sec,
            interactions_per_second: rates.interactions_per_sec,
            process_resident_memory_bytes: process_rss_bytes().unwrap_or(0),
            container_memory_used_bytes: read_u64_file("/sys/fs/cgroup/memory.current"),
            clickhouse_rows: clickhouse
                .as_ref()
                .map(|s| s.tables.iter().map(|t| t.rows).sum()),
            clickhouse_compressed_bytes: clickhouse
                .as_ref()
                .map(|s| s.tables.iter().map(|t| t.compressed_bytes).sum()),
        };
        let mut history = state.system_history.write().await;
        history.push_back(sample);
        while history.len() > MAX_SAMPLES {
            history.pop_front();
        }
    }
}

async fn system(State(s): State<AppState>) -> Json<SystemStats> {
    let (clickhouse, clickhouse_error) = match s.storage.stats().await {
        Ok(stats) => (Some(stats), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Json(SystemStats {
        scope_note: "load and host uptime use the container-visible /proc; memory uses this process and its cgroup; filesystem capacity is for the archive mount",
        runtime: RuntimeStats {
            process_uptime_seconds: s.growth_baseline.measured_at.elapsed().as_secs(),
            process_resident_memory_bytes: process_rss_bytes().unwrap_or(0),
            container_memory_used_bytes: read_u64_file("/sys/fs/cgroup/memory.current"),
            container_memory_limit_bytes: read_u64_file("/sys/fs/cgroup/memory.max"),
            process_cpu_seconds: process_cpu_seconds(),
            container_cpu_seconds: cgroup_cpu_seconds(),
            host_cpu_total_jiffies: host_cpu_jiffies().map(|v| v.0),
            host_cpu_idle_jiffies: host_cpu_jiffies().map(|v| v.1),
            host_load_average: load_average(),
            host_uptime_seconds: fs::read_to_string("/proc/uptime")
                .ok()
                .and_then(|v| v.split_whitespace().next()?.parse().ok()),
        },
        storage_array: FilesystemStats {
            path: s.archive_dir.display().to_string(),
            total_bytes: fs2::total_space(&s.archive_dir).unwrap_or(0),
            available_bytes: fs2::available_space(&s.archive_dir).unwrap_or(0),
            used_bytes: fs2::total_space(&s.archive_dir)
                .unwrap_or(0)
                .saturating_sub(fs2::available_space(&s.archive_dir).unwrap_or(0)),
        },
        clickhouse,
        clickhouse_error,
    })
}

fn read_u64_file(path: &str) -> Option<u64> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn process_rss_bytes() -> Option<u64> {
    let status = fs::read_to_string("/proc/self/status").ok()?;
    let kib = status
        .lines()
        .find(|line| line.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()?;
    Some(kib * 1024)
}

fn load_average() -> Option<[f64; 3]> {
    let raw = fs::read_to_string("/proc/loadavg").ok()?;
    let mut values = raw.split_whitespace().take(3).map(str::parse::<f64>);
    Some([
        values.next()?.ok()?,
        values.next()?.ok()?,
        values.next()?.ok()?,
    ])
}

fn process_cpu_seconds() -> Option<f64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    // The command name may contain spaces and parentheses, so fields begin after the final ')'.
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    // SAFETY: sysconf is a read-only libc query with no pointer arguments.
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    (ticks_per_second > 0).then(|| ticks as f64 / ticks_per_second as f64)
}

fn cgroup_cpu_seconds() -> Option<f64> {
    let stat = fs::read_to_string("/sys/fs/cgroup/cpu.stat").ok()?;
    let micros = stat.lines().find_map(|line| {
        let (name, value) = line.split_once(' ')?;
        (name == "usage_usec")
            .then(|| value.parse::<u64>().ok())
            .flatten()
    })?;
    Some(micros as f64 / 1_000_000.0)
}

fn host_cpu_jiffies() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let values: Vec<u64> = stat
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let total = values.iter().sum();
    let idle = *values.get(3)? + values.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_linux_process_metrics_without_panicking() {
        assert!(process_rss_bytes().is_some());
        assert!(load_average().is_some());
        assert!(process_cpu_seconds().is_some());
        assert!(host_cpu_jiffies().is_some());
    }

    #[test]
    fn assigns_safe_cache_policies() {
        assert_eq!(cache_policy("/api/stats"), "no-store");
        assert_eq!(
            cache_policy("/assets/app-abc.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_policy("/clusters/example"), "no-cache");
    }
}
