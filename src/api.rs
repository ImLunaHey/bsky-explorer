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
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware,
    middleware::Next,
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{get, get_service},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    convert::Infallible,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
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
    let spa = || get_service(ServeFile::new(index.clone()));
    let frontend = ServeDir::new(&s.web_dist_dir).not_found_service(ServeFile::new(index.clone()));
    Router::new()
        .route_service("/", spa())
        .route_service("/firehose", spa())
        .route_service("/detections", spa())
        .route_service("/system", spa())
        .route_service("/accounts/{did}", spa())
        .route_service("/records/{uri}", spa())
        .route_service("/clusters/{cluster_id}", spa())
        .route_service("/trends/{dimension}", spa())
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/api/stats", get(stats))
        .route("/api/firehose", get(firehose))
        .route("/api/live", get(live))
        .route("/api/trending/urls", get(urls))
        .route("/api/trending/domains", get(domains))
        .route("/api/trending/fingerprints", get(fingerprints))
        .route("/api/trending/drilldown", get(trend_drilldown))
        .route("/api/trending/conversations", get(conversations))
        .route("/api/anomalies", get(anomalies))
        .route("/api/clusters", get(clusters))
        .route("/api/clusters/{id}", get(cluster))
        .route("/api/accounts/{did}", get(account))
        .route("/api/records", get(record))
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

#[derive(Default, Deserialize)]
struct LiveQuery {
    after_time_us: Option<i64>,
}

async fn live(
    State(s): State<AppState>,
    Query(query): Query<LiveQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, (StatusCode, String)>
{
    let mut cursor = live_cursor(query.after_time_us, headers.get("last-event-id"))?;
    // Subscribe before taking the retained snapshot. Events arriving between
    // these operations are present in both sources and deduplicated by cursor.
    let mut receiver = s.analysis.subscribe_live();
    let analysis = s.analysis.clone();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("rates").json_data(analysis.rates()).expect("rates serialize"));
        let replay = analysis.recent_events(crate::analysis::RecentEventFilter {
            after_time_us: cursor,
            limit: 500,
            ..Default::default()
        });
        for event in replay.events {
            cursor = Some(cursor.map_or(event.time_us, |value| value.max(event.time_us)));
            yield Ok(live_event(&event));
        }

        let mut rates = tokio::time::interval(Duration::from_secs(1));
        rates.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        rates.tick().await;
        loop {
            tokio::select! {
                _ = rates.tick() => {
                    yield Ok(Event::default().event("rates").json_data(analysis.rates()).expect("rates serialize"));
                }
                message = receiver.recv() => match message {
                    Ok(event) => {
                        if cursor.is_none_or(|value| event.time_us > value) {
                            cursor = Some(event.time_us);
                            yield Ok(live_event(&event));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let replay = analysis.recent_events(crate::analysis::RecentEventFilter {
                            after_time_us: cursor,
                            limit: 500,
                            ..Default::default()
                        });
                        for event in replay.events {
                            cursor = Some(cursor.map_or(event.time_us, |value| value.max(event.time_us)));
                            yield Ok(live_event(&event));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

fn live_event(event: &crate::model::NormalizedEvent) -> Event {
    Event::default()
        .event("firehose")
        .id(event.time_us.to_string())
        .json_data(event)
        .expect("normalized event serialize")
}

fn live_cursor(
    query_cursor: Option<i64>,
    last_event_id: Option<&HeaderValue>,
) -> Result<Option<i64>, (StatusCode, String)> {
    let header_cursor = last_event_id
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::parse::<i64>)
        .transpose()
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "Last-Event-ID must be an integer time_us".into(),
            )
        })?;
    // A reconnect retains the original URL query while EventSource adds a
    // newer Last-Event-ID. Always resume from the furthest known position.
    let cursor = match (query_cursor, header_cursor) {
        (Some(query), Some(header)) => Some(query.max(header)),
        (query, header) => query.or(header),
    };
    if cursor.is_some_and(|value| value < 0) {
        return Err((
            StatusCode::BAD_REQUEST,
            "after_time_us cannot be negative".into(),
        ));
    }
    Ok(cursor)
}
#[derive(Deserialize)]
struct FirehoseQuery {
    #[serde(default = "default_firehose_limit")]
    limit: usize,
    kind: Option<String>,
    q: Option<String>,
    after_time_us: Option<i64>,
}

fn default_firehose_limit() -> usize {
    100
}

async fn firehose(
    State(s): State<AppState>,
    Query(query): Query<FirehoseQuery>,
) -> Result<Json<crate::analysis::RecentEvents>, (StatusCode, String)> {
    if query.limit == 0 || query.limit > 500 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 500".into(),
        ));
    }
    if query.after_time_us.is_some_and(|value| value < 0) {
        return Err((
            StatusCode::BAD_REQUEST,
            "after_time_us cannot be negative".into(),
        ));
    }
    if let Some(kind) = query.kind.as_deref().filter(|kind| !kind.is_empty()) {
        const KINDS: &[&str] = &[
            "post", "like", "repost", "follow", "unfollow", "reply", "quote", "delete", "other",
        ];
        if !KINDS.contains(&kind.to_ascii_lowercase().as_str()) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("unknown event kind: {kind}"),
            ));
        }
    }
    Ok(Json(s.analysis.recent_events(
        crate::analysis::RecentEventFilter {
            after_time_us: query.after_time_us,
            kind: query.kind,
            query: query.q,
            limit: query.limit,
        },
    )))
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
#[derive(Deserialize)]
struct TrendDrilldownQuery {
    dimension: String,
    key: String,
    #[serde(default = "default_drilldown_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
    kind: Option<String>,
}

fn default_drilldown_limit() -> usize {
    50
}

async fn trend_drilldown(
    State(s): State<AppState>,
    Query(query): Query<TrendDrilldownQuery>,
) -> Result<Json<crate::analysis::TrendDrilldown>, (StatusCode, String)> {
    if query.key.trim().is_empty() || query.key.len() > 2_048 {
        return Err((
            StatusCode::BAD_REQUEST,
            "key must be between 1 and 2048 bytes".into(),
        ));
    }
    if query.limit == 0 || query.limit > 200 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 200".into(),
        ));
    }
    if query.offset > 10_000 {
        return Err((StatusCode::BAD_REQUEST, "offset cannot exceed 10000".into()));
    }
    let kind = query
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if kind.is_some_and(|kind| !valid_event_kind(kind)) {
        return Err((StatusCode::BAD_REQUEST, "unknown event kind".into()));
    }
    let dimension = parse_trend_dimension(&query.dimension, &query.key)?;
    Ok(Json(s.analysis.trend_drilldown(
        dimension,
        query.key.trim(),
        kind,
        query.offset,
        query.limit,
    )))
}

fn parse_trend_dimension(
    dimension: &str,
    key: &str,
) -> Result<crate::analysis::TrendDimension, (StatusCode, String)> {
    match dimension {
        "domain" => Ok(crate::analysis::TrendDimension::Domain),
        "url" => Ok(crate::analysis::TrendDimension::Url),
        "fingerprint" if key.len() == 16 => u64::from_str_radix(key, 16)
            .map(crate::analysis::TrendDimension::Fingerprint)
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "fingerprint key must be a 16-character hexadecimal value".into(),
                )
            }),
        "fingerprint" => Err((
            StatusCode::BAD_REQUEST,
            "fingerprint key must be a 16-character hexadecimal value".into(),
        )),
        _ => Err((
            StatusCode::BAD_REQUEST,
            "dimension must be domain, url, or fingerprint".into(),
        )),
    }
}

fn valid_event_kind(kind: &str) -> bool {
    const KINDS: &[&str] = &[
        "post", "like", "repost", "follow", "unfollow", "reply", "quote", "delete", "other",
    ];
    KINDS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(kind))
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
) -> Result<Json<AccountDetail>, (StatusCode, String)> {
    if !did.starts_with("did:") {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    let mut activity = s.analysis.account_activity(&did);
    let current_identity = s
        .identities
        .resolve(&did)
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    let identity_history = s
        .storage
        .identity_history(&did, 50)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let stored = s
        .storage
        .account_stats(&did)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    for cluster_id in s
        .storage
        .account_cluster_ids(&did, 50)
        .await
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    {
        if !activity.cluster_ids.contains(&cluster_id) {
            activity.cluster_ids.push(cluster_id);
        }
    }
    Ok(Json(AccountDetail {
        current_identity,
        identity_history,
        activity,
        stored,
    }))
}

#[derive(Serialize)]
struct AccountDetail {
    current_identity: IdentityRecord,
    identity_history: Vec<IdentityRecord>,
    activity: crate::analysis::AccountActivity,
    /// Full-retention totals from ClickHouse. `activity` is the hot rolling view.
    stored: Option<crate::storage::AccountStoredStats>,
}

#[derive(Deserialize)]
struct RecordQuery {
    uri: String,
}

#[derive(Serialize)]
struct RecordDetail {
    record: crate::storage::StoredRecord,
    interaction_counts: crate::storage::RecordInteractionCounts,
    recent_interactions: Vec<crate::storage::StoredInteraction>,
    conversation: Vec<crate::storage::StoredRecord>,
}

async fn record(
    State(s): State<AppState>,
    Query(query): Query<RecordQuery>,
) -> Result<Json<RecordDetail>, (StatusCode, String)> {
    if !is_at_uri(&query.uri) {
        return Err((StatusCode::BAD_REQUEST, "uri must be a valid AT URI".into()));
    }
    let record = match s.storage.record(&query.uri).await.map_err(internal_error)? {
        Some(record) => record,
        None => s
            .analysis
            .retained_record(&query.uri)
            .map(|event| crate::storage::StoredRecord {
                at_uri: event.at_uri,
                actor_did: event.actor_did,
                timestamp_ms: event.timestamp.timestamp_millis(),
                kind: event.kind.as_str().to_owned(),
                text: event.text.unwrap_or_default(),
                reply_parent: event.reply_parent.unwrap_or_default(),
                reply_root: event.reply_root.unwrap_or_default(),
                quote_target: event.quote_target.unwrap_or_default(),
                urls: event.urls,
                domains: event.domains,
                languages: event.languages,
            })
            .ok_or_else(|| (StatusCode::NOT_FOUND, "record not retained".into()))?,
    };
    let root = if record.reply_root.is_empty() {
        &query.uri
    } else {
        &record.reply_root
    };
    let (interaction_counts, recent_interactions, mut conversation) = tokio::try_join!(
        s.storage.record_interaction_counts(&query.uri),
        s.storage.record_interactions(&query.uri, 100),
        s.storage.conversation_context(root, 200),
    )
    .map_err(internal_error)?;
    // Restarts may replay a small durable checkpoint interval. Hide those
    // append-only duplicates and do not repeat the primary record as context.
    conversation.retain(|item| item.at_uri != query.uri);
    let mut seen = std::collections::HashSet::new();
    conversation.retain(|item| seen.insert(item.at_uri.clone()));
    Ok(Json(RecordDetail {
        record,
        interaction_counts,
        recent_interactions,
        conversation,
    }))
}

fn is_at_uri(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("at://") else {
        return false;
    };
    let mut parts = rest.split('/');
    matches!(parts.next(), Some(authority) if authority.starts_with("did:") || authority.contains('.'))
        && matches!(parts.next(), Some(collection) if !collection.is_empty())
        && matches!(parts.next(), Some(rkey) if !rkey.is_empty())
        && parts.next().is_none()
}

fn internal_error(error: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
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
    process_cpu_utilization_percent: Option<f64>,
    container_cpu_utilization_percent: Option<f64>,
    host_cpu_utilization_percent: Option<f64>,
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
    process_cpu_utilization_percent: Option<f64>,
    container_cpu_utilization_percent: Option<f64>,
    host_cpu_utilization_percent: Option<f64>,
    #[serde(skip)]
    cpu_counters: CpuCounters,
}

#[derive(Clone, Copy, Default)]
struct CpuCounters {
    process_seconds: Option<f64>,
    container_seconds: Option<f64>,
    host_total_jiffies: Option<u64>,
    host_idle_jiffies: Option<u64>,
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
        let timestamp = chrono::Utc::now().timestamp_millis();
        let cpu_counters = read_cpu_counters();
        let mut history = state.system_history.write().await;
        let percentages = history
            .back()
            .map(|previous| {
                cpu_percentages(
                    previous.cpu_counters,
                    cpu_counters,
                    (timestamp - previous.timestamp) as f64 / 1_000.0,
                )
            })
            .unwrap_or_default();
        let sample = SystemSample {
            timestamp,
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
            process_cpu_utilization_percent: percentages.process,
            container_cpu_utilization_percent: percentages.container,
            host_cpu_utilization_percent: percentages.host,
            cpu_counters,
        };
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
    let latest_cpu = s
        .system_history
        .read()
        .await
        .back()
        .map(|sample| CpuPercentages {
            process: sample.process_cpu_utilization_percent,
            container: sample.container_cpu_utilization_percent,
            host: sample.host_cpu_utilization_percent,
        })
        .unwrap_or_default();
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
            process_cpu_utilization_percent: latest_cpu.process,
            container_cpu_utilization_percent: latest_cpu.container,
            host_cpu_utilization_percent: latest_cpu.host,
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

fn read_cpu_counters() -> CpuCounters {
    let host = host_cpu_jiffies();
    CpuCounters {
        process_seconds: process_cpu_seconds(),
        container_seconds: cgroup_cpu_seconds(),
        host_total_jiffies: host.map(|value| value.0),
        host_idle_jiffies: host.map(|value| value.1),
    }
}

#[derive(Default)]
struct CpuPercentages {
    process: Option<f64>,
    container: Option<f64>,
    host: Option<f64>,
}

fn cpu_percentages(previous: CpuCounters, current: CpuCounters, elapsed: f64) -> CpuPercentages {
    if elapsed <= 0.0 || !elapsed.is_finite() {
        return CpuPercentages::default();
    }
    let counter_percent = |before: Option<f64>, after: Option<f64>| match (before, after) {
        (Some(before), Some(after)) if after >= before => Some((after - before) / elapsed * 100.0),
        _ => None,
    };
    let host = match (
        previous.host_total_jiffies,
        previous.host_idle_jiffies,
        current.host_total_jiffies,
        current.host_idle_jiffies,
    ) {
        (Some(old_total), Some(old_idle), Some(new_total), Some(new_idle))
            if new_total > old_total && new_idle >= old_idle =>
        {
            let total = new_total - old_total;
            let idle = (new_idle - old_idle).min(total);
            Some((total - idle) as f64 / total as f64 * 100.0)
        }
        _ => None,
    };
    CpuPercentages {
        process: counter_percent(previous.process_seconds, current.process_seconds),
        container: counter_percent(previous.container_seconds, current.container_seconds),
        host,
    }
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

    #[test]
    fn live_cursor_uses_furthest_resume_position() {
        let header = HeaderValue::from_static("123");
        assert_eq!(live_cursor(None, Some(&header)).unwrap(), Some(123));
        assert_eq!(live_cursor(Some(456), Some(&header)).unwrap(), Some(456));
        assert_eq!(live_cursor(Some(100), Some(&header)).unwrap(), Some(123));
        assert!(live_cursor(Some(-1), None).is_err());
        assert!(live_cursor(None, Some(&HeaderValue::from_static("oops"))).is_err());
    }

    #[test]
    fn validates_record_at_uris() {
        assert!(is_at_uri("at://did:plc:alice/app.bsky.feed.post/3kexample"));
        assert!(is_at_uri("at://alice.test/app.bsky.feed.post/3kexample"));
        assert!(!is_at_uri("https://bsky.app/profile/alice.test/post/3k"));
        assert!(!is_at_uri("at://did:plc:alice/app.bsky.feed.post"));
        assert!(!is_at_uri(
            "at://did:plc:alice/app.bsky.feed.post/key/extra"
        ));
    }

    #[test]
    fn validates_trend_dimensions_and_fingerprints() {
        assert!(matches!(
            parse_trend_dimension("domain", "example.test"),
            Ok(crate::analysis::TrendDimension::Domain)
        ));
        assert!(matches!(
            parse_trend_dimension("fingerprint", "000000000000002a"),
            Ok(crate::analysis::TrendDimension::Fingerprint(42))
        ));
        assert!(parse_trend_dimension("fingerprint", "42").is_err());
        assert!(parse_trend_dimension("account", "did:plc:test").is_err());
        assert!(valid_event_kind("RePoSt"));
        assert!(!valid_event_kind("subscribe"));
    }

    #[test]
    fn calculates_cpu_percentages_and_rejects_counter_resets() {
        let previous = CpuCounters {
            process_seconds: Some(10.0),
            container_seconds: Some(20.0),
            host_total_jiffies: Some(1_000),
            host_idle_jiffies: Some(600),
        };
        let current = CpuCounters {
            process_seconds: Some(10.5),
            container_seconds: Some(21.0),
            host_total_jiffies: Some(1_200),
            host_idle_jiffies: Some(700),
        };
        let usage = cpu_percentages(previous, current, 2.0);
        assert_eq!(usage.process, Some(25.0));
        assert_eq!(usage.container, Some(50.0));
        assert_eq!(usage.host, Some(50.0));

        let reset = cpu_percentages(current, previous, 2.0);
        assert!(reset.process.is_none());
        assert!(reset.container.is_none());
        assert!(reset.host.is_none());
        assert!(cpu_percentages(previous, current, 0.0).host.is_none());
    }
}
