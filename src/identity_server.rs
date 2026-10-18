//! Single owner of identity lookup work and its durable display-name cache.
//! The ingester writes observations to ClickHouse, never waits on this service.
use crate::{
    api::{IdentityQueueQuery, queue_page},
    config::Config,
    identity::{IdentityRecord, IdentityService},
    identity_queue::{IdentityJob, IdentityQueue, IdentityQueuePage},
    storage::Storage,
};
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use fs2::FileExt;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Clone)]
struct Service {
    identities: IdentityService,
    queue: IdentityQueue,
    lookups: Arc<AtomicU64>,
    hits: Arc<AtomicU64>,
}
type Error = (StatusCode, String);
fn error(e: impl std::fmt::Display) -> Error {
    tracing::warn!(error=%e,"identity service request failed");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "identity service temporarily unavailable".into(),
    )
}

pub async fn run(cfg: Config) -> Result<()> {
    let root = cfg
        .identity_queue_dir
        .clone()
        .unwrap_or_else(|| cfg.data_dir.join("identity-queue"));
    std::fs::create_dir_all(&root)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join("service.lock"))?;
    lock.try_lock_exclusive()?;
    let storage = Storage::new(
        &cfg.clickhouse_url,
        &cfg.clickhouse_database,
        &cfg.clickhouse_user,
        &cfg.clickhouse_password,
    );
    storage.ensure_identity_service_schema().await?;
    let queue = IdentityQueue::open(
        root,
        cfg.identity_queue_capacity,
        chrono::Duration::hours(24),
    )?;
    let identities = IdentityService::new(
        storage.clone(),
        cfg.identity_profile_endpoint,
        cfg.identity_cache_capacity,
        Duration::from_secs(cfg.identity_cache_ttl_seconds),
        Duration::from_secs(cfg.identity_negative_ttl_seconds),
        cfg.identity_requests_per_second,
    )
    .with_persistence(queue.clone());
    let state = Service {
        identities: identities.clone(),
        queue: queue.clone(),
        lookups: Default::default(),
        hits: Default::default(),
    };
    tokio::spawn(observations(
        storage.clone(),
        identities.clone(),
        queue.clone(),
    ));
    let outbox_storage = storage.clone();
    let outbox_queue = queue.clone();
    tokio::spawn(async move {
        loop {
            let result = async {
                let records = outbox_queue.outbox()?;
                outbox_storage.insert_identities(&records).await?;
                outbox_queue.acknowledge_outbox(&records)?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = result {
                tracing::warn!(%error,"identity publication will retry from durable outbox");
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    tokio::spawn(async move {
        // Import existing mappings first, including explicit invalidations. The
        // API still works during import through one bounded ClickHouse batch.
        loop {
            match bootstrap(&storage, &identities, &queue).await {
                Ok(()) => break,
                Err(e) => tracing::warn!(error=%e,"identity import will resume"),
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        // One batched worker means a 429 cools down the entire upstream budget.
        tokio::spawn(crate::identity_queue::run_worker(
            queue.clone(),
            identities.clone(),
        ));
        loop {
            if let Err(e) = discover(&storage, &identities, &queue).await {
                tracing::warn!(error=%e,"identity backlog sweep will resume");
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    let app = Router::new()
        .route("/readyz", get(|| async { "ready" }))
        .route("/resolve", post(resolve))
        .route("/api/identity-queue", get(jobs))
        .route("/api/identity-queue/{id}", get(job))
        .route("/stats", get(stats))
        .layer(DefaultBodyLimit::max(32_768))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    tracing::info!(address=%cfg.listen_addr,"identity service listening");
    axum::serve(listener, app).await?;
    drop(lock);
    Ok(())
}

async fn resolve(
    State(s): State<Service>,
    Json(dids): Json<Vec<String>>,
) -> Result<Json<Vec<IdentityRecord>>, Error> {
    if dids.is_empty()
        || dids.len() > 100
        || dids.iter().any(|v| !crate::identity_queue::valid_did(v))
    {
        return Err((StatusCode::BAD_REQUEST, "provide 1..100 valid DIDs".into()));
    }
    let records = s
        .identities
        .resolve_local_batch(&dids)
        .await
        .map_err(error)?;
    s.lookups.fetch_add(records.len() as u64, Ordering::Relaxed);
    for record in &records {
        if record.source == "unresolved" {
            if let Err(e) = s.queue.enqueue(&record.did) {
                tracing::warn!(error=%e,"visible identity queue full");
            }
        } else {
            s.hits.fetch_add(1, Ordering::Relaxed);
        }
    }
    Ok(Json(records))
}
async fn jobs(
    State(s): State<Service>,
    Query(q): Query<IdentityQueueQuery>,
) -> Result<Json<IdentityQueuePage>, Error> {
    queue_page(&s.queue, q).map(Json)
}
async fn job(State(s): State<Service>, Path(id): Path<String>) -> Result<Json<IdentityJob>, Error> {
    if id.len() > 64 {
        return Err((StatusCode::BAD_REQUEST, "invalid job id".into()));
    }
    s.queue
        .get(&id)
        .map_err(error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "identity job not found".into()))
}
async fn stats(State(s): State<Service>) -> Result<Json<serde_json::Value>, Error> {
    Ok(Json(
        serde_json::json!({"display_lookups":s.lookups.load(Ordering::Relaxed),"known_display_lookups":s.hits.load(Ordering::Relaxed),"queue":s.queue.summary(),"bootstrap_complete":s.queue.progress("bootstrap_done").map_err(error)?=="yes","backlog_cursor":s.queue.progress("backlog").map_err(error)?,"observations_checkpoint_ms":s.queue.progress("observations").map_err(error)?}),
    ))
}

async fn bootstrap(
    storage: &Storage,
    identities: &IdentityService,
    queue: &IdentityQueue,
) -> Result<()> {
    if queue.progress("bootstrap_done")? == "yes" {
        return Ok(());
    }
    loop {
        let rows = storage
            .identity_bootstrap_page(&queue.progress("bootstrap")?)
            .await?;
        if rows.is_empty() {
            queue.set_progress("bootstrap_done", "yes")?;
            return Ok(());
        }
        identities.accept_observations(&rows)?;
        queue.set_progress("bootstrap", &rows.last().unwrap().did)?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn observations(storage: Storage, identities: IdentityService, queue: IdentityQueue) {
    loop {
        if let Err(e) = observation_pass(&storage, &identities, &queue).await {
            tracing::warn!(error=%e,"identity observation tail will resume");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
async fn observation_pass(
    storage: &Storage,
    identities: &IdentityService,
    queue: &IdentityQueue,
) -> Result<()> {
    // Replay overlap handles concurrent inserts and a crash between write and
    // checkpoint. Timestamp-guarded upserts make replay idempotent.
    let mut ms = queue
        .progress("observations")?
        .parse::<i64>()
        .unwrap_or(0)
        .saturating_sub(60_000);
    let mut did = String::new();
    loop {
        let rows = storage.identity_change_page(ms, &did).await?;
        if rows.is_empty() {
            return Ok(());
        }
        let last = rows.last().unwrap();
        ms = last.received_ms;
        did = last.did.clone();
        identities.accept_observations(
            &rows
                .into_iter()
                .filter_map(|r| r.record())
                .collect::<Vec<_>>(),
        )?;
        queue.set_progress("observations", &ms.to_string())?;
    }
}
async fn discover(
    storage: &Storage,
    identities: &IdentityService,
    queue: &IdentityQueue,
) -> Result<()> {
    let summary = queue.summary();
    // Bound discovery ahead of consumption, reserving room for visible users.
    if summary.queued + summary.retrying + summary.running >= 5000 {
        return Ok(());
    }
    let dids = storage
        .identity_backlog_page(&queue.progress("backlog")?)
        .await?;
    if dids.is_empty() {
        queue.set_progress("backlog", "")?;
        return Ok(());
    }
    let valid = dids
        .iter()
        .filter(|did| crate::identity_queue::valid_did(did))
        .cloned()
        .collect::<Vec<_>>();
    for chunk in valid.chunks(100) {
        for record in identities.resolve_local_batch(chunk).await? {
            if record.source == "unresolved" {
                queue.enqueue_priority(&record.did, false)?;
            }
        }
    }
    queue.set_progress("backlog", dids.last().unwrap())?;
    Ok(())
}
