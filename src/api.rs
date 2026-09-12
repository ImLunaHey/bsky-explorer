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
    routing::{get, get_service, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    convert::Infallible,
    fs,
    path::PathBuf,
    sync::Arc,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::{RwLock, Semaphore};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
};
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
    pub storage_budget_bytes: u64,
    pub system_history: SystemHistory,
    pub identities: IdentityService,
    pub backfills: crate::backfill::BackfillQueue,
    pub identity_queue: crate::identity_queue::IdentityQueue,
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
        .route_service("/coverage", spa())
        .route_service("/search", spa())
        .route_service("/backfills", spa())
        .route_service("/backfills/{id}", spa())
        .route_service("/crawls/{id}", spa())
        .route_service("/communities", spa())
        .route_service("/communities/{id}", spa())
        .route_service("/identities", spa())
        .route_service("/identities/{id}", spa())
        .route_service("/accounts/{did}", spa())
        .route_service("/records/{uri}", spa())
        .route_service("/clusters/{cluster_id}", spa())
        .route_service("/trends/{dimension}", spa())
        .route("/healthz", get(health))
        .route("/readyz", get(ready))
        .route("/metrics", get(metrics))
        .route("/api/stats", get(stats))
        .route("/api/search", get(search))
        .route("/api/backfills", get(backfills).post(enqueue_backfill))
        .route("/api/backfills/{id}/status", get(backfill_status))
        .route("/api/backfills/{id}/events", get(backfill_events))
        .route("/api/backfills/{id}/cancel", post(backfill_cancel))
        .route("/api/backfills/{id}/retry", post(backfill_retry))
        .route("/api/backfills/{id}/pause", post(backfill_pause))
        .route("/api/backfills/{id}/resume", post(backfill_resume))
        .route("/api/crawls", get(crawls).post(create_crawl))
        .route("/api/crawls/{id}", get(crawl_detail))
        .route("/api/crawls/{id}/pause", post(crawl_pause))
        .route("/api/crawls/{id}/resume", post(crawl_resume))
        .route("/api/crawls/{id}/cancel", post(crawl_cancel))
        .route("/api/communities", get(communities))
        .route("/api/communities/{id}", get(community_detail))
        .route("/api/firehose", get(firehose))
        .route("/api/live", get(live))
        .route("/api/trending/urls", get(urls))
        .route("/api/trending/domains", get(domains))
        .route("/api/trending/fingerprints", get(fingerprints))
        .route("/api/trending/accounts", get(trending_accounts))
        .route("/api/trending/drilldown", get(trend_drilldown))
        .route("/api/trending/conversations", get(conversations))
        .route("/api/anomalies", get(anomalies))
        .route("/api/clusters", get(clusters))
        .route("/api/clusters/{id}", get(cluster))
        .route("/api/accounts/{did}", get(account))
        .route("/api/accounts/{did}/profile", get(account_profile))
        .route("/api/accounts/{did}/events", get(account_events))
        .route("/api/accounts/{did}/follows", get(account_follows))
        .route("/api/accounts/{did}/analytics", get(account_analytics))
        .route("/api/accounts/{did}/lexicons", get(account_lexicons))
        .route("/api/records", get(record))
        .route("/api/records/media", get(record_media))
        .route("/api/records/media/batch", post(record_media_batch))
        .route("/api/identities", get(identities))
        .route("/api/identity-queue", get(identity_queue))
        .route("/api/identity-queue/{id}", get(identity_queue_detail))
        .route("/api/storage", get(storage))
        .route("/api/coverage", get(coverage))
        .route("/api/system", get(system))
        .route("/api/system/history", get(system_history))
        .fallback_service(frontend)
        .layer(CompressionLayer::new())
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
    Json(analyzer_blocking(s.analysis, |analysis| analysis.rates()).await)
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_search_type", rename = "type")]
    result_type: String,
    #[serde(default = "default_search_limit")]
    limit: u64,
    #[serde(default)]
    offset: u64,
}

fn default_search_type() -> String {
    "all".into()
}

const fn default_search_limit() -> u64 {
    25
}

#[derive(Debug, Serialize)]
struct SearchResponse {
    query: String,
    #[serde(rename = "type")]
    result_type: String,
    offset: u64,
    limit: u64,
    has_more: bool,
    results: Vec<SearchResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    backfill_job: Option<crate::backfill::BackfillJob>,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    result_type: &'static str,
    did: String,
    handle: Option<String>,
    event_id: Option<String>,
    at_uri: Option<String>,
    event_type: Option<String>,
    collection: Option<String>,
    operation: Option<String>,
    timestamp_ms: Option<i64>,
    text: Option<String>,
    record_json: Option<String>,
    target: Option<String>,
    urls: Vec<String>,
    domains: Vec<String>,
}

async fn search(
    State(s): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let requested_term = query.q.trim();
    validate_search_query(
        requested_term,
        &query.result_type,
        query.limit,
        query.offset,
    )?;
    let term = normalized_search_term(requested_term);
    let limit = query.limit.min(100);
    let fetch_limit = query.offset + limit + 1;
    let include_accounts = matches!(query.result_type.as_str(), "all" | "account");
    let (identities, events) = tokio::time::timeout(Duration::from_millis(2_500), async {
        // Identity lookup also turns a handle into the DID used by event rows.
        let identities = s.storage.search_identities(&term, fetch_limit).await?;
        let event_term = identities
            .iter()
            .find(|identity| {
                identity.handle_value.eq_ignore_ascii_case(&term) || identity.did == term
            })
            .map(|identity| identity.did.as_str())
            .unwrap_or(&term);
        let events = match query.result_type.as_str() {
            "record" => {
                s.storage
                    .search_events(
                        event_term,
                        crate::storage::SearchEventType::Records,
                        fetch_limit,
                    )
                    .await?
            }
            "event" => {
                s.storage
                    .search_events(
                        event_term,
                        crate::storage::SearchEventType::Events,
                        fetch_limit,
                    )
                    .await?
            }
            "all" => {
                // Put authored content ahead of generic interactions so a busy
                // like history cannot bury the account's posts.
                let (mut records, interactions) = tokio::try_join!(
                    s.storage.search_events(
                        event_term,
                        crate::storage::SearchEventType::Records,
                        fetch_limit
                    ),
                    s.storage.search_events(
                        event_term,
                        crate::storage::SearchEventType::Events,
                        fetch_limit
                    )
                )?;
                records.extend(interactions);
                records
            }
            "account" => Vec::new(),
            _ => unreachable!("validated above"),
        };
        anyhow::Ok((identities, events))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::REQUEST_TIMEOUT,
            "search timed out; use a more specific term".into(),
        )
    })?
    .map_err(internal_error)?;

    let mut results = Vec::with_capacity(identities.len() + events.len());
    if include_accounts {
        results.extend(identities.into_iter().map(|identity| SearchResult {
            result_type: "account",
            did: identity.did,
            handle: (!identity.handle_value.is_empty()).then_some(identity.handle_value),
            event_id: None,
            at_uri: None,
            event_type: None,
            collection: None,
            operation: None,
            timestamp_ms: None,
            text: None,
            record_json: None,
            target: None,
            urls: Vec::new(),
            domains: Vec::new(),
        }));
    }
    results.extend(events.into_iter().map(|event| {
        let is_record = matches!(event.kind.as_str(), "post" | "reply" | "quote");
        SearchResult {
            result_type: if is_record { "record" } else { "event" },
            did: event.actor_did,
            handle: None,
            event_id: Some(event.event_id),
            at_uri: (!event.at_uri.is_empty()).then_some(event.at_uri),
            event_type: Some(event.kind),
            collection: Some(event.collection),
            operation: Some(event.operation),
            timestamp_ms: Some(event.timestamp_ms),
            text: (!event.text.is_empty()).then_some(event.text),
            record_json: (!event.record_json.is_empty()).then_some(event.record_json),
            target: (!event.target.is_empty()).then_some(event.target),
            urls: event.urls,
            domains: event.domains,
        }
    }));
    let start = usize::try_from(query.offset)
        .unwrap_or(usize::MAX)
        .min(results.len());
    let mut page: Vec<_> = results
        .into_iter()
        .skip(start)
        .take(limit as usize + 1)
        .collect();
    let has_more = page.len() > limit as usize;
    page.truncate(limit as usize);
    let result_dids: Vec<String> = page
        .iter()
        .filter(|result| result.handle.is_none())
        .map(|result| result.did.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let result_handles: HashMap<_, _> = s
        .storage
        .identities_for_dids(&result_dids)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|identity| (identity.did, identity.handle_value))
        .collect();
    for result in &mut page {
        if result.handle.is_none() {
            result.handle = result_handles
                .get(&result.did)
                .filter(|handle| !handle.is_empty())
                .cloned();
        }
    }
    let backfill_job = if page.is_empty()
        && (requested_term.starts_with("did:") || requested_term.contains('.'))
    {
        s.backfills
            .enqueue(
                requested_term,
                term.starts_with("did:").then_some(term.as_str()),
                crate::backfill::JobType::Repository,
                None,
                true,
            )
            .ok()
    } else {
        None
    };
    Ok(Json(SearchResponse {
        query: requested_term.to_owned(),
        result_type: query.result_type,
        offset: query.offset,
        limit,
        has_more,
        results: page,
        backfill_job,
    }))
}

#[derive(Deserialize)]
struct BackfillListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
    status: Option<String>,
    job_type: Option<String>,
}
async fn backfills(
    State(s): State<AppState>,
    Query(q): Query<BackfillListQuery>,
) -> Json<serde_json::Value> {
    let limit = q.limit.unwrap_or(25).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).min(10_000);
    let (jobs, total) = s
        .backfills
        .list_page(limit, offset, q.status.as_deref(), q.job_type.as_deref())
        .unwrap_or_else(|error| {
            tracing::error!(%error,"list backfills");
            (vec![], 0)
        });
    let has_more = offset + jobs.len() < total;
    Json(
        serde_json::json!({"jobs":jobs,"total":total,"offset":offset,"limit":limit,"has_more":has_more}),
    )
}
#[derive(Deserialize)]
struct EnqueueBackfill {
    query: String,
    did: Option<String>,
    include_associations: Option<bool>,
    job_type: Option<String>,
}
async fn enqueue_backfill(
    State(s): State<AppState>,
    Json(body): Json<EnqueueBackfill>,
) -> Result<Json<crate::backfill::BackfillJob>, (StatusCode, String)> {
    let q = body.query.trim();
    if !crate::backfill::valid_backfill_query(q)
        || body.did.as_ref().is_some_and(|d| {
            !d.starts_with("did:plc:") || !crate::backfill::valid_backfill_query(d)
        })
    {
        return Err((StatusCode::BAD_REQUEST, "invalid backfill identity".into()));
    }
    let kind = match body.job_type.as_deref().unwrap_or("repository") {
        "repository" => crate::backfill::JobType::Repository,
        "associations" => crate::backfill::JobType::Associations,
        _ => return Err((StatusCode::BAD_REQUEST, "invalid job_type".into())),
    };
    s.backfills
        .enqueue(
            q,
            body.did.as_deref(),
            kind,
            None,
            body.include_associations.unwrap_or(true),
        )
        .map(Json)
        .map_err(internal_error)
}
#[derive(Deserialize)]
struct CreateCrawl {
    root: String,
    did: Option<String>,
    max_depth: u8,
    relations: Vec<String>,
    limits: Option<crate::backfill::CrawlLimits>,
}
async fn create_crawl(
    State(s): State<AppState>,
    Json(body): Json<CreateCrawl>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (crawl, job) = s
        .backfills
        .create_crawl(
            body.root.trim(),
            body.did.as_deref(),
            body.max_depth,
            body.relations,
            body.limits.unwrap_or_default(),
        )
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({"crawl":crawl,"root_job":job})))
}
#[derive(Deserialize)]
struct CrawlList {
    limit: Option<usize>,
    offset: Option<usize>,
    status: Option<String>,
}
async fn crawls(
    State(s): State<AppState>,
    Query(q): Query<CrawlList>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let limit = q.limit.unwrap_or(25).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).min(1_000_000);
    let (rows, total) = s
        .backfills
        .list_crawls(limit, offset, q.status.as_deref())
        .map_err(internal_error)?;
    Ok(Json(
        serde_json::json!({"crawls":rows,"total":total,"limit":limit,"offset":offset,"has_more":offset+rows.len()<total}),
    ))
}
async fn crawl_detail(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    s.backfills
        .crawl(&id)
        .map_err(internal_error)?
        .map(|(crawl, jobs)| Json(serde_json::json!({"crawl":crawl,"jobs":jobs})))
        .ok_or((StatusCode::NOT_FOUND, "crawl not found".into()))
}
async fn crawl_action(
    s: AppState,
    id: String,
    action: &str,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let affected = s
        .backfills
        .control_crawl(&id, action)
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(serde_json::json!({"affected":affected})))
}
async fn crawl_pause(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    crawl_action(s, id, "pause").await
}
async fn crawl_resume(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    crawl_action(s, id, "resume").await
}
async fn crawl_cancel(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    crawl_action(s, id, "cancel").await
}
#[derive(Deserialize)]
struct CommunityQuery {
    window: Option<u64>,
    min_size: Option<u64>,
    sort: Option<String>,
    order: Option<String>,
    limit: Option<u64>,
    offset: Option<u64>,
    edge_limit: Option<u64>,
    min_edge: Option<f64>,
}
async fn communities(
    State(s): State<AppState>,
    Query(q): Query<CommunityQuery>,
) -> Result<Json<crate::storage::CommunityOverview>, (StatusCode, String)> {
    let window = q.window.unwrap_or(86400);
    if !matches!(window, 3600 | 86400 | 604800) {
        return Err((
            StatusCode::BAD_REQUEST,
            "window must be 3600, 86400, or 604800".into(),
        ));
    }
    let min_size = q.min_size.unwrap_or(2).clamp(1, 10_000);
    let sort = q.sort.as_deref().unwrap_or("score");
    if !matches!(sort, "score" | "account_count" | "event_count" | "density") {
        return Err((StatusCode::BAD_REQUEST, "invalid sort".into()));
    }
    let limit = q.limit.unwrap_or(25).clamp(1, 100);
    let offset = q.offset.unwrap_or(0).min(1_000_000);
    s.storage
        .community_overview(
            window,
            min_size,
            sort,
            q.order.as_deref() != Some("asc"),
            limit,
            offset,
            q.edge_limit.unwrap_or(200).clamp(1, 500),
            q.min_edge.unwrap_or(0.0).max(0.0),
        )
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            "no completed community snapshot".into(),
        ))
}
#[derive(Deserialize)]
struct CommunityDetailQuery {
    snapshot_id: Option<String>,
    member_limit: Option<u64>,
    member_offset: Option<u64>,
    edge_limit: Option<u64>,
    min_edge: Option<f64>,
}
async fn community_detail(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CommunityDetailQuery>,
) -> Result<Json<crate::storage::CommunityDetail>, (StatusCode, String)> {
    if id.len() > 128 || q.snapshot_id.as_ref().is_some_and(|v| v.len() > 128) {
        return Err((StatusCode::BAD_REQUEST, "invalid community id".into()));
    }
    s.storage
        .community_detail(
            &id,
            q.snapshot_id.as_deref(),
            q.member_limit.unwrap_or(50).clamp(1, 200),
            q.member_offset.unwrap_or(0).min(1_000_000),
            q.edge_limit.unwrap_or(200).clamp(1, 500),
            q.min_edge.unwrap_or(0.0).max(0.0),
        )
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "community not found".into()))
}
#[derive(Deserialize)]
struct ControlScope {
    scope: Option<String>,
}
async fn job_action(
    s: AppState,
    id: String,
    q: ControlScope,
    action: &str,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let affected = s
        .backfills
        .control(&id, action, q.scope.as_deref().unwrap_or("job"))
        .map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(serde_json::json!({"affected":affected})))
}
async fn backfill_pause(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ControlScope>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    job_action(s, id, q, "pause").await
}
async fn backfill_resume(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ControlScope>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    job_action(s, id, q, "resume").await
}
#[derive(Serialize)]
struct BackfillProgress {
    kind: String,
    updated_at: DateTime<Utc>,
    collection_index: Option<u64>,
    current_collection: Option<String>,
    cursor: Option<String>,
    records_written: u64,
    posts_processed: Option<u64>,
    posts_completed: Option<u64>,
    posts_total: Option<u64>,
    posts_remaining: Option<u64>,
    requests: Option<u64>,
    likes: Option<u64>,
    reposts: Option<u64>,
    quotes: Option<u64>,
    replies: Option<u64>,
    errors: Option<u64>,
    rate_per_second: Option<f64>,
    eta_seconds: Option<u64>,
}
#[derive(Serialize)]
struct BackfillDetail {
    #[serde(flatten)]
    job: crate::backfill::BackfillJob,
    progress: Option<BackfillProgress>,
}
async fn backfill_status(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    if !crate::backfill::valid_job_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid job id").into_response();
    }
    match s.backfills.get(&id) {
        Some(job) => {
            let checkpoint = s.backfills.checkpoint(&id);
            let posts_total = if matches!(job.job_type, crate::backfill::JobType::Associations) {
                if let Some(total) = checkpoint
                    .as_ref()
                    .and_then(|checkpoint| checkpoint.payload.get("posts_total"))
                    .and_then(serde_json::Value::as_u64)
                    .filter(|total| *total > 0)
                {
                    Some(total)
                } else if let Some(did) = job.did.as_deref() {
                    s.storage.account_post_count(did).await.ok()
                } else {
                    None
                }
            } else {
                None
            };
            let progress = checkpoint.map(|checkpoint| {
                let value = &checkpoint.payload;
                let get = |key: &str| value.get(key).and_then(serde_json::Value::as_u64);
                let processed = get("next_post");
                let elapsed = (checkpoint.updated_at - job.created_at)
                    .num_milliseconds()
                    .max(1) as f64
                    / 1000.0;
                let rate = processed
                    .or_else(|| get("records_written"))
                    .map(|n| n as f64 / elapsed)
                    .filter(|n| *n > 0.0);
                let remaining = posts_total
                    .zip(processed)
                    .map(|(total, done)| total.saturating_sub(done));
                BackfillProgress {
                    kind: checkpoint.kind,
                    updated_at: checkpoint.updated_at,
                    collection_index: get("collection_index"),
                    current_collection: value
                        .get("collection")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    cursor: value
                        .get("cursor")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    records_written: get("records_written").unwrap_or(job.records_written),
                    posts_processed: processed,
                    posts_completed: get("posts_completed"),
                    posts_total,
                    posts_remaining: remaining,
                    requests: get("requests"),
                    likes: get("likes"),
                    reposts: get("reposts"),
                    quotes: get("quotes"),
                    replies: get("replies"),
                    errors: get("errors"),
                    rate_per_second: rate,
                    eta_seconds: remaining
                        .zip(rate)
                        .map(|(left, per_second)| (left as f64 / per_second).ceil() as u64),
                }
            });
            Json(BackfillDetail { job, progress }).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
#[derive(Deserialize)]
struct BackfillEventsQuery {
    limit: Option<u64>,
    offset: Option<u64>,
}
async fn backfill_events(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BackfillEventsQuery>,
) -> impl IntoResponse {
    if !crate::backfill::valid_job_id(&id) {
        return (StatusCode::BAD_REQUEST, "invalid job id").into_response();
    }
    let Some(job) = s.backfills.get(&id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(did) = job.did.as_deref() else {
        return (StatusCode::CONFLICT, "job DID is unresolved").into_response();
    };
    match s
        .storage
        .backfill_events(
            did,
            matches!(job.job_type, crate::backfill::JobType::Associations),
            query.limit.unwrap_or(50),
            query.offset.unwrap_or(0),
        )
        .await
    {
        Ok((events, total)) => Json(
            serde_json::json!({"events":events,"total":total,"offset":query.offset.unwrap_or(0)}),
        )
        .into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}
async fn backfill_cancel(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ControlScope>,
) -> impl IntoResponse {
    match s
        .backfills
        .control(&id, "cancel", q.scope.as_deref().unwrap_or("job"))
    {
        Ok(affected) => Json(serde_json::json!({"affected":affected})).into_response(),
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}
async fn backfill_retry(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match s.backfills.retry(&id) {
        Ok(Some(j)) => Json(j).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::CONFLICT, e.to_string()).into_response(),
    }
}

fn normalized_search_term(term: &str) -> String {
    let term = term.strip_prefix('@').unwrap_or(term);
    if term.len() == 24
        && term
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        format!("did:plc:{term}")
    } else {
        term.to_owned()
    }
}

fn validate_search_query(
    term: &str,
    result_type: &str,
    limit: u64,
    offset: u64,
) -> Result<(), (StatusCode, String)> {
    if !(2..=500).contains(&term.len()) || term.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            "q must contain 2-500 printable characters".into(),
        ));
    }
    if !matches!(result_type, "all" | "account" | "record" | "event") {
        return Err((
            StatusCode::BAD_REQUEST,
            "type must be all, account, record, or event".into(),
        ));
    }
    if !(1..=100).contains(&limit) {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 100".into(),
        ));
    }
    if offset > 10_000 {
        return Err((
            StatusCode::BAD_REQUEST,
            "offset must not exceed 10000".into(),
        ));
    }
    Ok(())
}

async fn analyzer_blocking<T: Send + 'static>(
    analysis: Analyzer,
    operation: impl FnOnce(&Analyzer) -> T + Send + 'static,
) -> T {
    tokio::task::spawn_blocking(move || operation(&analysis))
        .await
        .expect("analyzer worker panicked")
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
        let initial_analysis = analysis.clone();
        let initial_rates = analyzer_blocking(initial_analysis, |a| a.rates()).await;
        yield Ok(Event::default().event("rates").json_data(initial_rates).expect("rates serialize"));
        let replay_analysis = analysis.clone();
        let replay = analyzer_blocking(replay_analysis, move |a| a.recent_events(crate::analysis::RecentEventFilter {
            after_time_us: cursor,
            limit: 500,
            ..Default::default()
        })).await;
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
                    let rates_analysis = analysis.clone();
                    let current_rates = analyzer_blocking(rates_analysis, |a| a.rates()).await;
                    yield Ok(Event::default().event("rates").json_data(current_rates).expect("rates serialize"));
                }
                message = receiver.recv() => match message {
                    Ok(event) => {
                        if cursor.is_none_or(|value| event.time_us > value) {
                            cursor = Some(event.time_us);
                            yield Ok(live_event(&event));
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        let replay_analysis = analysis.clone();
                        let replay = analyzer_blocking(replay_analysis, move |a| a.recent_events(crate::analysis::RecentEventFilter {
                            after_time_us: cursor,
                            limit: 500,
                            ..Default::default()
                        })).await;
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
    let mut event = event.clone();
    // The compact record remains replayable in ClickHouse/archive, but sending
    // up to 64 KiB with every live frame is unnecessary and made reconnect
    // frames inconsistent with genuinely live ones.
    event.record_json = None;
    Event::default()
        .event("firehose")
        .id(event.time_us.to_string())
        .json_data(&event)
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

#[derive(Deserialize)]
struct TrendingAccountsQuery {
    metric: Option<String>,
    window: Option<String>,
    limit: Option<u64>,
}
#[derive(Serialize)]
struct TrendingAccountsResponse {
    metric: String,
    window: String,
    window_seconds: u64,
    accounts: Vec<crate::storage::TrendingAccount>,
}
async fn trending_accounts(
    State(s): State<AppState>,
    Query(q): Query<TrendingAccountsQuery>,
) -> Result<Json<TrendingAccountsResponse>, (StatusCode, String)> {
    let metric = q.metric.unwrap_or_else(|| "followers".into());
    if !matches!(
        metric.as_str(),
        "followers" | "reposts" | "likes" | "replies" | "posts"
    ) {
        return Err((StatusCode::BAD_REQUEST, "invalid metric".into()));
    }
    let window = q.window.unwrap_or_else(|| "24h".into());
    let window_seconds = trending_account_window(&window).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "window must be 1h, 24h, or 7d".into(),
        )
    })?;
    let limit = q.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be between 1 and 100".into(),
        ));
    }
    let accounts = s
        .storage
        .trending_accounts(&metric, window_seconds, limit)
        .await
        .map_err(internal_error)?;
    Ok(Json(TrendingAccountsResponse {
        metric,
        window,
        window_seconds,
        accounts,
    }))
}
fn trending_account_window(value: &str) -> Option<u64> {
    match value {
        "1h" => Some(3600),
        "24h" => Some(86400),
        "7d" => Some(604800),
        _ => None,
    }
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
#[derive(Serialize)]
struct EnrichedConversation {
    key: String,
    count: u64,
    recent_count: u64,
    growth: i64,
    sample: Option<String>,
    record: Option<crate::storage::StoredRecord>,
    handle: Option<String>,
}

async fn conversations(
    State(s): State<AppState>,
) -> Result<Json<Vec<EnrichedConversation>>, (StatusCode, String)> {
    let items = analyzer_blocking(s.analysis.clone(), |analysis| {
        analysis.growing_conversations()
    })
    .await;
    let roots: Vec<String> = items
        .iter()
        .map(|item| item.key.clone())
        .filter(|uri| is_bluesky_post_uri(uri))
        .take(20)
        .collect();
    let local = s.storage.records(&roots).await.map_err(internal_error)?;
    let mut records: HashMap<String, crate::storage::StoredRecord> = HashMap::new();
    for record in local {
        records.entry(record.at_uri.clone()).or_insert(record);
    }
    let missing: Vec<String> = roots
        .iter()
        .filter(|uri| !records.contains_key(*uri))
        .cloned()
        .collect();
    if !missing.is_empty() {
        match fetch_appview_posts(&missing).await {
            Ok(fetched) => {
                if let Err(error) = s.storage.insert_events(&fetched).await {
                    tracing::warn!(%error, "could not persist AppView conversation records");
                }
                for event in fetched {
                    let record = stored_record_from_event(event);
                    records.insert(record.at_uri.clone(), record);
                }
            }
            Err((status, error)) => {
                tracing::warn!(%status, %error, "could not enrich conversation roots");
            }
        }
    }
    let dids: Vec<String> = records
        .values()
        .map(|record| record.actor_did.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let handles: HashMap<_, _> = s
        .storage
        .identities_for_dids(&dids)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|identity| (identity.did, identity.handle_value))
        .collect();
    Ok(Json(
        items
            .into_iter()
            .map(|item| {
                let record = records.remove(&item.key);
                let handle = record
                    .as_ref()
                    .and_then(|record| handles.get(&record.actor_did))
                    .filter(|handle| !handle.is_empty())
                    .cloned();
                let sample = item.sample.or_else(|| {
                    record
                        .as_ref()
                        .map(|record| record.text.chars().take(180).collect())
                });
                EnrichedConversation {
                    key: item.key,
                    count: item.count,
                    recent_count: item.recent_count,
                    growth: item.growth,
                    sample,
                    record,
                    handle,
                }
            })
            .collect(),
    ))
}
async fn anomalies(State(s): State<AppState>) -> Json<crate::analysis::CategorizedAnomalies> {
    Json(s.analysis.categorized_anomalies())
}
async fn clusters(State(s): State<AppState>) -> Json<Vec<crate::model::Detection>> {
    Json(s.analysis.detections())
}
async fn cluster(State(s): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    match s.storage.detection(&id).await {
        Ok(Some(detection)) => {
            Json(crate::analysis::ClusterDetail::from_persisted(detection)).into_response()
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => internal_error(error).into_response(),
    }
}
async fn account(
    State(s): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<AccountDetail>, (StatusCode, String)> {
    if !did.starts_with("did:") {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    let (current_identity, identity_history, stored, stored_clusters) = tokio::try_join!(
        s.identities.resolve_local(&did),
        s.storage.identity_history(&did, 50),
        s.storage.account_stats(&did),
        s.storage.account_cluster_ids(&did, 50),
    )
    .map_err(internal_error)?;
    if current_identity.handle.is_none()
        && let Err(error) = s.identity_queue.enqueue(&did)
    {
        tracing::warn!(%error, %did, "account identity enrichment was not queued");
    }
    // Account pages must not contend with the ingest writer for the global hot
    // analyzer lock. ClickHouse is indexed for this lookup and remains responsive
    // while the collector is catching up.
    let activity = account_activity_from_storage(&did, stored.as_ref(), stored_clusters);
    Ok(Json(AccountDetail {
        current_identity,
        identity_history,
        activity,
        stored,
    }))
}

fn account_activity_from_storage(
    did: &str,
    stored: Option<&crate::storage::AccountStoredStats>,
    cluster_ids: Vec<String>,
) -> crate::analysis::AccountActivity {
    use crate::analysis::{AccountActivity, AccountSummary};
    use crate::model::ScoreBreakdown;

    let (events, posts, targets, first_seen, last_seen, domains, urls) =
        stored.map_or((0, 0, 0, None, None, Vec::new(), Vec::new()), |row| {
            (
                row.event_count as usize,
                row.post_count as usize,
                row.unique_targets as usize,
                chrono::DateTime::from_timestamp_millis(row.first_seen_ms),
                chrono::DateTime::from_timestamp_millis(row.last_seen_ms),
                row.domains.clone(),
                row.urls.clone(),
            )
        });
    AccountActivity {
        summary: AccountSummary {
            did: did.to_owned(),
            events_24h: events,
            posts_24h: posts,
            unique_targets: targets,
            automation_score: 0.0,
        },
        first_seen,
        last_seen,
        windows: Vec::new(),
        unique_domains: domains,
        unique_urls: urls,
        recent_events: Vec::new(),
        cluster_ids,
        scores: ScoreBreakdown {
            automation_score: 0.0,
            coordination_score: 0.0,
            shared_target_score: 0.0,
            timing_similarity_score: 0.0,
            content_similarity_score: 0.0,
        },
    }
}

async fn account_events(
    State(s): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<Vec<crate::storage::StoredAccountEvent>>, (StatusCode, String)> {
    if !did.starts_with("did:") {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    s.storage
        .account_recent_events(&did, 100)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn account_follows(
    State(s): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<crate::storage::AccountFollowAnalytics>, (StatusCode, String)> {
    if !did.starts_with("did:") {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    s.storage
        .account_follow_analytics(&did)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn account_analytics(
    State(s): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<crate::storage::AccountAnalytics>, (StatusCode, String)> {
    if !did.starts_with("did:") {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    s.storage
        .account_analytics(&did)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn account_lexicons(
    State(s): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<crate::storage::AccountLexiconInventory>, (StatusCode, String)> {
    if !did.starts_with("did:") {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    s.storage
        .account_lexicon_inventory(&did)
        .await
        .map(Json)
        .map_err(internal_error)
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
        None => match s.analysis.retained_record(&query.uri) {
            Some(event) => stored_record_from_event(event),
            None => {
                let mut fetched = fetch_appview_posts(std::slice::from_ref(&query.uri)).await?;
                let event = fetched
                    .pop()
                    .ok_or_else(|| (StatusCode::NOT_FOUND, "post was not found".into()))?;
                s.storage
                    .insert_events(std::slice::from_ref(&event))
                    .await
                    .map_err(internal_error)?;
                stored_record_from_event(event)
            }
        },
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

fn stored_record_from_event(event: crate::model::NormalizedEvent) -> crate::storage::StoredRecord {
    crate::storage::StoredRecord {
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
    }
}

fn is_bluesky_post_uri(uri: &str) -> bool {
    is_at_uri(uri) && uri.contains("/app.bsky.feed.post/")
}

async fn fetch_appview_posts(
    uris: &[String],
) -> Result<Vec<crate::model::NormalizedEvent>, (StatusCode, String)> {
    const MAX_URIS: usize = 20;
    const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
    if uris.is_empty() {
        return Ok(Vec::new());
    }
    if uris.len() > MAX_URIS
        || uris
            .iter()
            .any(|uri| !is_bluesky_post_uri(uri) || uri.len() > 512)
    {
        return Err((StatusCode::BAD_REQUEST, "invalid AppView post batch".into()));
    }
    let requested: HashSet<&str> = uris.iter().map(String::as_str).collect();
    if requested.len() != uris.len() {
        return Err((StatusCode::BAD_REQUEST, "duplicate post URI".into()));
    }
    let query: Vec<(&str, &str)> = uris.iter().map(|uri| ("uris", uri.as_str())).collect();
    let response = media_client()
        .get("https://public.api.bsky.app/xrpc/app.bsky.feed.getPosts")
        .query(&query)
        .send()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?
        .error_for_status()
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_RESPONSE_BYTES as u64)
    {
        return Err((
            StatusCode::BAD_GATEWAY,
            "AppView response is too large".into(),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err((
            StatusCode::BAD_GATEWAY,
            "AppView response is too large".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    let posts = value
        .get("posts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                "AppView response has no posts".into(),
            )
        })?;
    if posts.len() > MAX_URIS {
        return Err((
            StatusCode::BAD_GATEWAY,
            "AppView returned too many posts".into(),
        ));
    }
    let observed_at = chrono::Utc::now();
    let mut seen = HashSet::new();
    posts
        .iter()
        .map(|post| {
            let uri = post
                .get("uri")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| (StatusCode::BAD_GATEWAY, "AppView post has no URI".into()))?;
            if !requested.contains(uri) || !seen.insert(uri) {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "AppView returned an unexpected post".into(),
                ));
            }
            crate::normalize::normalize_appview_post(post, observed_at)
                .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))
        })
        .collect()
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct RecordMedia {
    kind: String,
    url: String,
    thumbnail_url: Option<String>,
    alt: String,
    width: Option<u64>,
    height: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct RecordMediaBatchItem {
    uri: String,
    media: Vec<RecordMedia>,
    found: bool,
}

#[derive(Deserialize)]
struct RecordMediaBatchRequest {
    uris: Vec<String>,
}

#[derive(Serialize)]
struct RecordMediaBatchResponse {
    items: Vec<RecordMediaBatchItem>,
    source: &'static str,
    ephemeral: bool,
}

#[derive(Clone)]
struct CachedRecordMedia {
    fetched_at: Instant,
    item: RecordMediaBatchItem,
}

static MEDIA_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static MEDIA_CACHE: OnceLock<RwLock<HashMap<String, CachedRecordMedia>>> = OnceLock::new();
static MEDIA_UPSTREAM_LIMIT: OnceLock<Semaphore> = OnceLock::new();
const MEDIA_CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_MEDIA_BATCH_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

fn media_client() -> &'static reqwest::Client {
    MEDIA_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(4))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("bsky-observatory/0.1")
            .build()
            .expect("static AppView client configuration is valid")
    })
}

fn validated_media_uris(uris: Vec<String>) -> Result<Vec<String>, (StatusCode, String)> {
    if uris.is_empty() || uris.len() > 25 {
        return Err((
            StatusCode::BAD_REQUEST,
            "uris must contain between 1 and 25 post URIs".into(),
        ));
    }
    let mut seen = HashSet::with_capacity(uris.len());
    let mut unique = Vec::with_capacity(uris.len());
    for uri in uris {
        if uri.len() > 512 || !is_bluesky_post_uri(&uri) {
            return Err((StatusCode::BAD_REQUEST, "invalid Bluesky post URI".into()));
        }
        if seen.insert(uri.clone()) {
            unique.push(uri);
        }
    }
    Ok(unique)
}

async fn record_media_batch(
    Json(body): Json<RecordMediaBatchRequest>,
) -> Result<Json<RecordMediaBatchResponse>, (StatusCode, String)> {
    let uris = validated_media_uris(body.uris)?;
    let now = Instant::now();
    let cache = MEDIA_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut items = HashMap::with_capacity(uris.len());
    {
        let cached = cache.read().await;
        for uri in &uris {
            if let Some(entry) = cached
                .get(uri)
                .filter(|entry| now.duration_since(entry.fetched_at) < MEDIA_CACHE_TTL)
            {
                items.insert(uri.clone(), entry.item.clone());
            }
        }
    }
    let missing: Vec<_> = uris
        .iter()
        .filter(|uri| !items.contains_key(*uri))
        .cloned()
        .collect();
    if !missing.is_empty() {
        let _permit = MEDIA_UPSTREAM_LIMIT
            .get_or_init(|| Semaphore::new(4))
            .acquire()
            .await
            .map_err(|_| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "media service unavailable".into(),
                )
            })?;
        let response = media_client()
            .get("https://public.api.bsky.app/xrpc/app.bsky.feed.getPosts")
            .query(
                &missing
                    .iter()
                    .map(|uri| ("uris", uri.as_str()))
                    .collect::<Vec<_>>(),
            )
            .send()
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "AppView rate limited media metadata".into(),
            ));
        }
        let response = response
            .error_for_status()
            .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_MEDIA_BATCH_RESPONSE_BYTES as u64)
        {
            return Err((
                StatusCode::BAD_GATEWAY,
                "upstream metadata is too large".into(),
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
        if bytes.len() > MAX_MEDIA_BATCH_RESPONSE_BYTES {
            return Err((
                StatusCode::BAD_GATEWAY,
                "upstream metadata is too large".into(),
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
        let posts = value
            .get("posts")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                (
                    StatusCode::BAD_GATEWAY,
                    "AppView response has no posts".into(),
                )
            })?;
        if posts.len() > missing.len() {
            return Err((
                StatusCode::BAD_GATEWAY,
                "AppView returned too many posts".into(),
            ));
        }
        let mut returned = HashMap::with_capacity(posts.len());
        for post in posts {
            let uri = post
                .get("uri")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| (StatusCode::BAD_GATEWAY, "AppView post has no URI".into()))?;
            if !missing.iter().any(|expected| expected == uri) || returned.contains_key(uri) {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    "AppView returned an unexpected post".into(),
                ));
            }
            let media = post
                .get("embed")
                .map(extract_record_media)
                .unwrap_or_default();
            returned.insert(
                uri.to_owned(),
                RecordMediaBatchItem {
                    uri: uri.to_owned(),
                    media,
                    found: true,
                },
            );
        }
        let mut cached = cache.write().await;
        if cached.len() >= 10_000 {
            cached.retain(|_, entry| now.duration_since(entry.fetched_at) < MEDIA_CACHE_TTL);
            if cached.len() >= 10_000 {
                cached.clear();
            }
        }
        for uri in missing {
            let item = returned
                .get(&uri)
                .cloned()
                .unwrap_or_else(|| RecordMediaBatchItem {
                    uri: uri.clone(),
                    media: Vec::new(),
                    found: false,
                });
            cached.insert(
                uri.clone(),
                CachedRecordMedia {
                    fetched_at: now,
                    item: item.clone(),
                },
            );
            items.insert(uri, item);
        }
    }
    Ok(Json(RecordMediaBatchResponse {
        items: uris
            .into_iter()
            .filter_map(|uri| items.remove(&uri))
            .collect(),
        source: "public_appview",
        ephemeral: true,
    }))
}

#[derive(Serialize)]
struct RecordMediaResponse {
    media: Vec<RecordMedia>,
    source: &'static str,
    ephemeral: bool,
}

/// Fetches only AppView metadata. Blob bytes remain between the user's browser
/// and Bluesky's CDN and are never proxied, cached, or persisted here.
async fn record_media(
    Query(query): Query<RecordQuery>,
) -> Result<Json<RecordMediaResponse>, (StatusCode, String)> {
    if !is_at_uri(&query.uri) || !query.uri.contains("/app.bsky.feed.post/") {
        return Err((
            StatusCode::BAD_REQUEST,
            "uri must identify a Bluesky post".into(),
        ));
    }
    let response = media_client()
        .get("https://public.api.bsky.app/xrpc/app.bsky.feed.getPosts")
        .query(&[("uris", &query.uri)])
        .send()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?
        .error_for_status()
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if response
        .content_length()
        .is_some_and(|size| size > 1_048_576)
    {
        return Err((
            StatusCode::BAD_GATEWAY,
            "upstream metadata is too large".into(),
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if bytes.len() > 1_048_576 {
        return Err((
            StatusCode::BAD_GATEWAY,
            "upstream metadata is too large".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    let post = value
        .get("posts")
        .and_then(serde_json::Value::as_array)
        .and_then(|posts| posts.first())
        .filter(|post| post.get("uri").and_then(serde_json::Value::as_str) == Some(&query.uri));
    let media = post
        .and_then(|post| post.get("embed"))
        .map(extract_record_media)
        .unwrap_or_default();
    Ok(Json(RecordMediaResponse {
        media,
        source: "public_appview",
        ephemeral: true,
    }))
}

fn extract_record_media(embed: &serde_json::Value) -> Vec<RecordMedia> {
    let mut output = Vec::new();
    extract_record_media_into(embed, &mut output);
    output.truncate(8);
    output
}

fn extract_record_media_into(embed: &serde_json::Value, output: &mut Vec<RecordMedia>) {
    if output.len() >= 8 {
        return;
    }
    let kind = embed
        .get("$type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if kind.contains("recordWithMedia") {
        if let Some(media) = embed.get("media") {
            extract_record_media_into(media, output);
        }
        return;
    }
    if kind.contains("images") {
        for image in embed
            .get("images")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            if output.len() >= 8 {
                break;
            }
            let Some(url) = image
                .get("fullsize")
                .and_then(serde_json::Value::as_str)
                .filter(|url| valid_media_url(url))
            else {
                continue;
            };
            let thumbnail_url = image
                .get("thumb")
                .and_then(serde_json::Value::as_str)
                .filter(|url| valid_media_url(url))
                .map(str::to_owned);
            let ratio = image.get("aspectRatio");
            output.push(RecordMedia {
                kind: "image".into(),
                url: url.into(),
                thumbnail_url,
                alt: image
                    .get("alt")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .chars()
                    .take(2_000)
                    .collect(),
                width: ratio
                    .and_then(|v| v.get("width"))
                    .and_then(serde_json::Value::as_u64),
                height: ratio
                    .and_then(|v| v.get("height"))
                    .and_then(serde_json::Value::as_u64),
            });
        }
    } else if kind.contains("video")
        && let Some(url) = embed
            .get("playlist")
            .and_then(serde_json::Value::as_str)
            .filter(|url| valid_media_url(url))
    {
        let ratio = embed.get("aspectRatio");
        output.push(RecordMedia {
            kind: "video".into(),
            url: url.into(),
            thumbnail_url: embed
                .get("thumbnail")
                .and_then(serde_json::Value::as_str)
                .filter(|url| valid_media_url(url))
                .map(str::to_owned),
            alt: embed
                .get("alt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(2_000)
                .collect(),
            width: ratio
                .and_then(|v| v.get("width"))
                .and_then(serde_json::Value::as_u64),
            height: ratio
                .and_then(|v| v.get("height"))
                .and_then(serde_json::Value::as_u64),
        });
    }
}

fn valid_media_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.port().is_none()
        && matches!(url.host_str(), Some("cdn.bsky.app" | "video.bsky.app"))
        && url.username().is_empty()
        && url.password().is_none()
}

const PROFILE_CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_PROFILE_BYTES: usize = 64 * 1024;
static PROFILE_CACHE: OnceLock<RwLock<HashMap<String, CachedProfile>>> = OnceLock::new();

#[derive(Clone)]
struct CachedProfile {
    fetched_at: Instant,
    profile: AccountProfile,
}

#[derive(Debug, Clone, Serialize)]
struct AccountProfile {
    did: String,
    handle: String,
    display_name: Option<String>,
    description: Option<String>,
    avatar_url: Option<String>,
    labels: Vec<String>,
    followers_count: Option<u64>,
    follows_count: Option<u64>,
    posts_count: Option<u64>,
    indexed_at: Option<String>,
    observed_at: chrono::DateTime<chrono::Utc>,
}

async fn account_profile(
    Path(did): Path<String>,
) -> Result<Json<AccountProfile>, (StatusCode, String)> {
    if !valid_profile_did(&did) {
        return Err((StatusCode::BAD_REQUEST, "invalid DID".into()));
    }
    let cache = PROFILE_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if let Some(profile) = cache
        .read()
        .await
        .get(&did)
        .filter(|entry| entry.fetched_at.elapsed() < PROFILE_CACHE_TTL)
        .map(|entry| entry.profile.clone())
    {
        return Ok(Json(profile));
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("bsky-observatory/0.1")
        .build()
        .map_err(|error| internal_error(error.into()))?;
    let mut response = client
        .get("https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile")
        .query(&[("actor", &did)])
        .send()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?
        .error_for_status()
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_PROFILE_BYTES as u64)
    {
        return Err((
            StatusCode::BAD_GATEWAY,
            "profile metadata is too large".into(),
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_PROFILE_BYTES {
            return Err((
                StatusCode::BAD_GATEWAY,
                "profile metadata is too large".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if value.get("did").and_then(serde_json::Value::as_str) != Some(&did) {
        return Err((
            StatusCode::BAD_GATEWAY,
            "AppView returned a different DID".into(),
        ));
    }
    let avatar_url = value
        .get("avatar")
        .and_then(serde_json::Value::as_str)
        .filter(|url| valid_avatar_url(url))
        .map(str::to_owned);
    let profile = AccountProfile {
        did: did.clone(),
        handle: value
            .get("handle")
            .and_then(serde_json::Value::as_str)
            .filter(|handle| handle.len() <= 253)
            .unwrap_or(&did)
            .to_owned(),
        display_name: bounded_profile_string(&value, "displayName", 640),
        description: bounded_profile_string(&value, "description", 4096),
        avatar_url,
        labels: value
            .get("labels")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|label| label.get("val").and_then(serde_json::Value::as_str))
            .filter(|label| label.len() <= 128)
            .take(20)
            .map(str::to_owned)
            .collect(),
        followers_count: value
            .get("followersCount")
            .and_then(serde_json::Value::as_u64),
        follows_count: value
            .get("followsCount")
            .and_then(serde_json::Value::as_u64),
        posts_count: value.get("postsCount").and_then(serde_json::Value::as_u64),
        indexed_at: bounded_profile_string(&value, "indexedAt", 64),
        observed_at: chrono::Utc::now(),
    };
    cache.write().await.insert(
        did,
        CachedProfile {
            fetched_at: Instant::now(),
            profile: profile.clone(),
        },
    );
    Ok(Json(profile))
}

fn bounded_profile_string(value: &serde_json::Value, key: &str, max: usize) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|text| text.len() <= max)
        .map(str::to_owned)
}

fn valid_profile_did(did: &str) -> bool {
    (7..=256).contains(&did.len())
        && did.starts_with("did:")
        && did.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'.' | b'_' | b'-' | b'%')
        })
}

fn valid_avatar_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && url.port().is_none()
        && url.host_str() == Some("cdn.bsky.app")
        && url.username().is_empty()
        && url.password().is_none()
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
        let identity = s
            .identities
            .resolve_local(did)
            .await
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        if identity.handle.is_none()
            && let Err(error) = s.identity_queue.enqueue(did)
        {
            tracing::warn!(%error, %did, "visible identity enrichment was not queued");
        }
        resolved.push(identity);
    }
    Ok(Json(resolved))
}

#[derive(Deserialize)]
struct IdentityQueueQuery {
    limit: Option<usize>,
    #[serde(default)]
    offset: usize,
    status: Option<String>,
    q: Option<String>,
    sort: Option<String>,
    order: Option<String>,
    hours: Option<u32>,
    bucket_minutes: Option<u32>,
}
async fn identity_queue(
    State(s): State<AppState>,
    Query(q): Query<IdentityQueueQuery>,
) -> Result<Json<crate::identity_queue::IdentityQueuePage>, (StatusCode, String)> {
    if q.status.as_ref().is_some_and(|v| {
        !matches!(
            v.as_str(),
            "queued" | "running" | "retrying" | "resolved" | "negative" | "dead_letter"
        )
    }) {
        return Err((StatusCode::BAD_REQUEST, "invalid status".into()));
    }
    let descending = q.order.as_deref() != Some("asc");
    s.identity_queue
        .page(crate::identity_queue::IdentityPageQuery {
            limit: q.limit.unwrap_or(25).clamp(1, 100),
            offset: q.offset.min(1_000_000),
            status: q.status.as_deref(),
            search: q.q.as_deref().filter(|v| !v.is_empty()),
            sort: q.sort.as_deref().unwrap_or("updated_at"),
            descending,
            hours: q.hours.unwrap_or(24),
            bucket_minutes: q.bucket_minutes.unwrap_or(15),
        })
        .map(Json)
        .map_err(internal_error)
}
async fn identity_queue_detail(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::identity_queue::IdentityJob>, (StatusCode, String)> {
    if id.len() > 64 {
        return Err((StatusCode::BAD_REQUEST, "invalid job id".into()));
    }
    s.identity_queue
        .get(&id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "identity job not found".into()))
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
    storage_budget_bytes: u64,
    used_footprint_bytes: u64,
    remaining_allocation_bytes: u64,
    physical_available_bytes: u64,
    allocation_runway_days: Option<f64>,
    allocation_runway_months: Option<f64>,
    allocation_runway_years: Option<f64>,
    physical_runway_days: Option<f64>,
    physical_runway_months: Option<f64>,
    physical_runway_years: Option<f64>,
    limiting_factor: &'static str,
}
fn runway_days(remaining_bytes: u64, daily_growth: f64) -> Option<f64> {
    (daily_growth > 0.0 && daily_growth.is_finite()).then(|| remaining_bytes as f64 / daily_growth)
}
async fn coverage(
    State(s): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    static CACHE: OnceLock<RwLock<Option<(Instant, serde_json::Value)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| RwLock::new(None));
    if let Some((stored_at, value)) = &*cache.read().await
        && stored_at.elapsed() < Duration::from_secs(60)
    {
        return Ok(Json(value.clone()));
    }
    let queue = s.backfills.repository_coverage().map_err(internal_error)?;
    let (totals, collections, daily) = s.storage.coverage().await.map_err(internal_error)?;
    let value = serde_json::json!({
        "summary":{
            "unique_observed_actors":totals.unique_observed_actors,
            "identities_resolved":totals.identities_resolved,
            "repository_accounts_completed":queue.completed,
            "repository_jobs_queued":queue.queued,
            "repository_jobs_running":queue.running,
            "repository_jobs_partial":queue.partial,
            "repository_jobs_failed":queue.failed,
            "unique_records":totals.unique_records,
            "total_bytes":totals.total_bytes,
            "coverage_estimate":serde_json::Value::Null,
            "coverage_denominator":serde_json::Value::Null,
            "coverage_qualification":"Estimated distinct counts and retained-data coverage for this observatory; collection bytes are allocated proportionally. Total ATProto network size is not known."
        },"collections":collections,"daily":daily
    });
    *cache.write().await = Some((Instant::now(), value.clone()));
    Ok(Json(value))
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
    let daily = hourly * 24.0;
    let remaining_allocation = s.storage_budget_bytes.saturating_sub(current);
    let physical_available = fs2::available_space(&s.archive_dir).unwrap_or(0);
    let allocation_days = runway_days(remaining_allocation, daily);
    let physical_days = runway_days(physical_available, daily);
    let limiting_factor = match (allocation_days, physical_days) {
        (Some(allocation), Some(physical)) if allocation <= physical => "observatory_allocation",
        (Some(_), Some(_)) => "physical_array",
        _ => "growth_unavailable",
    };
    Json(StorageStats {
        raw_archive_bytes: raw,
        clickhouse_bytes: ch,
        derived_bytes: derived,
        bytes_ingested_per_hour: hourly,
        bytes_ingested_per_day: daily,
        projected_monthly_bytes: daily * 30.0,
        projected_yearly_bytes: daily * 365.0,
        storage_budget_bytes: s.storage_budget_bytes,
        used_footprint_bytes: current,
        remaining_allocation_bytes: remaining_allocation,
        physical_available_bytes: physical_available,
        allocation_runway_days: allocation_days,
        allocation_runway_months: allocation_days.map(|days| days / 30.4375),
        allocation_runway_years: allocation_days.map(|days| days / 365.25),
        physical_runway_days: physical_days,
        physical_runway_months: physical_days.map(|days| days / 30.4375),
        physical_runway_years: physical_days.map(|days| days / 365.25),
        limiting_factor,
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
    fn only_post_at_uris_are_eligible_for_appview_enrichment() {
        assert!(is_bluesky_post_uri(
            "at://did:plc:alice/app.bsky.feed.post/3kexample"
        ));
        assert!(is_bluesky_post_uri(
            "at://alice.test/app.bsky.feed.post/3kexample"
        ));
        assert!(!is_bluesky_post_uri(
            "at://did:plc:alice/app.bsky.feed.like/3kexample"
        ));
        assert!(!is_bluesky_post_uri("https://example.test/post"));
    }

    #[test]
    fn media_batches_are_bounded_validated_and_deduplicated() {
        let uri = "at://did:plc:alice/app.bsky.feed.post/3kexample".to_owned();
        assert_eq!(
            validated_media_uris(vec![uri.clone(), uri.clone()]).unwrap(),
            vec![uri]
        );
        assert!(validated_media_uris(Vec::new()).is_err());
        assert!(validated_media_uris(vec!["https://example.test/image".into()]).is_err());
        assert!(
            validated_media_uris(
                (0..26)
                    .map(|index| format!("at://did:plc:alice/app.bsky.feed.post/{index}"))
                    .collect()
            )
            .is_err()
        );
    }

    #[test]
    fn validates_bounded_search_requests() {
        assert!(validate_search_query("alice", "all", 25, 0).is_ok());
        assert!(validate_search_query("did:plc:alice", "account", 100, 10_000).is_ok());
        assert!(validate_search_query("x", "all", 25, 0).is_err());
        assert!(validate_search_query("alice\n", "all", 25, 0).is_err());
        assert!(validate_search_query("alice", "unknown", 25, 0).is_err());
        assert!(validate_search_query("alice", "all", 0, 0).is_err());
        assert!(validate_search_query("alice", "all", 101, 0).is_err());
        assert!(validate_search_query("alice", "all", 25, 10_001).is_err());
        assert_eq!(
            normalized_search_term("k6acu4chiwkixvdedcmdgmal"),
            "did:plc:k6acu4chiwkixvdedcmdgmal"
        );
        assert_eq!(normalized_search_term("@imlunahey.com"), "imlunahey.com");
        assert_eq!(normalized_search_term("ordinary words"), "ordinary words");
    }

    #[test]
    fn extracts_only_allowlisted_ephemeral_media() {
        let embed = serde_json::json!({
            "$type": "app.bsky.embed.images#view",
            "images": [
                {"fullsize":"https://cdn.bsky.app/img/feed_fullsize/plain/did:plc:test/cid@jpeg", "thumb":"https://cdn.bsky.app/img/feed_thumbnail/plain/did:plc:test/cid@jpeg", "alt":"description", "aspectRatio":{"width":1200,"height":800}},
                {"fullsize":"https://127.0.0.1/private", "alt":"unsafe"},
                {"fullsize":"https://cdn.bsky.app.evil.test/image", "alt":"lookalike"}
            ]
        });
        assert_eq!(
            extract_record_media(&embed),
            vec![RecordMedia {
                kind: "image".into(),
                url: "https://cdn.bsky.app/img/feed_fullsize/plain/did:plc:test/cid@jpeg".into(),
                thumbnail_url: Some(
                    "https://cdn.bsky.app/img/feed_thumbnail/plain/did:plc:test/cid@jpeg".into()
                ),
                alt: "description".into(),
                width: Some(1200),
                height: Some(800),
            }]
        );
        assert!(!valid_media_url("http://cdn.bsky.app/image"));
        assert!(!valid_media_url("https://user@cdn.bsky.app/image"));
    }

    #[test]
    fn extracts_video_from_record_with_media() {
        let embed = serde_json::json!({
            "$type":"app.bsky.embed.recordWithMedia#view",
            "media":{"$type":"app.bsky.embed.video#view", "playlist":"https://video.bsky.app/watch/did/cid/playlist.m3u8", "thumbnail":"https://video.bsky.app/watch/did/cid/thumbnail.jpg"}
        });
        let media = extract_record_media(&embed);
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].kind, "video");
    }

    #[test]
    fn validates_profile_identifiers_and_avatar_hosts() {
        assert!(valid_profile_did("did:plc:k6acu4chiwkixvdedcmdgmal"));
        assert!(!valid_profile_did("did:plc:test/../../etc/passwd"));
        assert!(!valid_profile_did("https://example.test"));
        assert!(valid_avatar_url(
            "https://cdn.bsky.app/img/avatar/plain/did:plc:test/cid@jpeg"
        ));
        assert!(!valid_avatar_url("http://cdn.bsky.app/image"));
        assert!(!valid_avatar_url("https://cdn.bsky.app.evil.test/image"));
        assert!(!valid_avatar_url("https://user@cdn.bsky.app/image"));
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
    fn validates_trending_account_windows() {
        assert_eq!(trending_account_window("1h"), Some(3_600));
        assert_eq!(trending_account_window("24h"), Some(86_400));
        assert_eq!(trending_account_window("7d"), Some(604_800));
        assert_eq!(trending_account_window("30d"), None);
        assert_eq!(trending_account_window("1h OR 1=1"), None);
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

    #[test]
    fn storage_runway_requires_positive_growth_and_uses_remaining_bytes() {
        assert_eq!(runway_days(1_000, 100.0), Some(10.0));
        assert_eq!(runway_days(0, 100.0), Some(0.0));
        assert_eq!(runway_days(1_000, 0.0), None);
        assert_eq!(runway_days(1_000, f64::NAN), None);
    }
}
