use anyhow::Result;
use bsky_observatory::{
    analysis::Analyzer,
    api::{self, AppState, GrowthBaseline},
    archive::Archive,
    config::Config,
    identity::IdentityService,
    metrics::Metrics,
    storage::Storage,
};
use clap::Parser;
use tracing_subscriber::EnvFilter;
// The ingest/parser pipeline can saturate a core while catching up from a
// cursor. Keep independent scheduler capacity for HTTP/SSE and ClickHouse I/O.
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let cfg = Config::parse();
    let metrics = Metrics::new()?;
    let storage = Storage::new(
        &cfg.clickhouse_url,
        &cfg.clickhouse_database,
        &cfg.clickhouse_user,
        &cfg.clickhouse_password,
    );
    let analysis = Analyzer::with_windows(
        cfg.coordination_window_seconds,
        cfg.min_cluster_accounts,
        cfg.min_cluster_events,
        cfg.state_retention_hours,
        cfg.windows_seconds.clone(),
    )
    .with_detail_limits(cfg.hot_detail_retention_seconds, cfg.hot_detail_max_events);
    let identities = IdentityService::new(
        storage.clone(),
        cfg.identity_profile_endpoint.clone(),
        cfg.identity_cache_capacity,
        std::time::Duration::from_secs(cfg.identity_cache_ttl_seconds),
        std::time::Duration::from_secs(cfg.identity_negative_ttl_seconds),
        cfg.identity_requests_per_second,
    );
    let backfill_dir = cfg
        .backfill_queue_dir
        .clone()
        .unwrap_or_else(|| cfg.data_dir.join("backfill-queue"));
    let backfills = bsky_observatory::backfill::BackfillQueue::open(backfill_dir)?;
    for _ in 0..cfg.backfill_worker_concurrency.clamp(1, 4) {
        tokio::spawn(bsky_observatory::backfill::run_worker(
            backfills.clone(),
            storage.clone(),
        ));
    }
    let identity_queue = bsky_observatory::identity_queue::IdentityQueue::open(
        cfg.identity_queue_dir
            .clone()
            .unwrap_or_else(|| cfg.data_dir.join("identity-queue")),
        cfg.identity_queue_capacity,
        chrono::Duration::seconds(cfg.identity_negative_ttl_seconds as i64),
    )?;
    for _ in 0..cfg.identity_worker_concurrency.clamp(1, 4) {
        tokio::spawn(bsky_observatory::identity_queue::run_worker(
            identity_queue.clone(),
            identities.clone(),
        ));
    }
    let state = AppState {
        analysis: analysis.clone(),
        metrics: metrics.clone(),
        storage: storage.clone(),
        archive_dir: cfg.data_dir.clone(),
        web_dist_dir: cfg.web_dist_dir.clone(),
        growth_baseline: GrowthBaseline {
            measured_at: std::time::Instant::now(),
            bytes: Archive::total_size(&cfg.data_dir) + storage.bytes().await.unwrap_or(0),
        },
        storage_budget_bytes: cfg.effective_storage_budget_bytes(),
        system_history: Default::default(),
        identities: identities.clone(),
        backfills,
        identity_queue: identity_queue.clone(),
    };
    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    tokio::spawn(api::sample_system_history(state.clone()));
    tokio::spawn(bsky_observatory::ingest::run(
        cfg, storage, analysis, metrics, identities,
    ));
    axum::serve(listener, api::router(state)).await?;
    Ok(())
}
