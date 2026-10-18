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
use std::collections::BTreeSet;
use tracing_subscriber::EnvFilter;
// The ingest/parser pipeline can saturate a core while catching up from a
// cursor. Keep independent scheduler capacity for HTTP/SSE and ClickHouse I/O.
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let cfg = Config::parse();
    let runs_workers = cfg.service_role != "api";
    let ingester_url = if cfg.service_role == "api" {
        let origin = cfg
            .ingester_url
            .clone()
            .ok_or_else(|| anyhow::anyhow!("API role requires INGESTER_URL for live views"))?;
        let parsed = url::Url::parse(&origin)?;
        anyhow::ensure!(
            matches!(parsed.scheme(), "http" | "https")
                && parsed.host_str().is_some()
                && parsed.path() == "/"
                && parsed.query().is_none()
                && parsed.fragment().is_none(),
            "INGESTER_URL must be an HTTP origin"
        );
        Some(origin)
    } else {
        None
    };
    let metrics = Metrics::new()?;
    let storage = Storage::new(
        &cfg.clickhouse_url,
        &cfg.clickhouse_database,
        &cfg.clickhouse_user,
        &cfg.clickhouse_password,
    );
    storage.ensure_community_schema().await?;
    if runs_workers {
        tokio::spawn(bsky_observatory::storage::snapshots::refresh_coverage(
            storage.clone(),
        ));
        tokio::spawn(bsky_observatory::storage::snapshots::refresh(
            storage.clone(),
        ));
        let community_configs = cfg
            .community_windows_seconds
            .iter()
            .copied()
            .filter(|window| *window > 0)
            .map(
                |window_seconds| bsky_observatory::community::CommunityConfig {
                    window_seconds,
                    event_types: cfg
                        .community_event_types
                        .iter()
                        .cloned()
                        .collect::<BTreeSet<_>>(),
                    min_edge_weight: cfg.community_min_edge_weight,
                    max_nodes: cfg.community_max_nodes,
                    max_edges: cfg.community_max_edges,
                    ..Default::default()
                },
            )
            .collect();
        tokio::spawn(bsky_observatory::community::run_periodically(
            storage.clone(),
            community_configs,
            cfg.community_max_input_rows,
            std::time::Duration::from_secs(cfg.community_interval_seconds),
        ));
    }
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
    for _ in 0..if runs_workers {
        cfg.backfill_worker_concurrency.clamp(1, 4)
    } else {
        0
    } {
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
    for _ in 0..if runs_workers {
        cfg.identity_worker_concurrency.clamp(1, 4)
    } else {
        0
    } {
        tokio::spawn(bsky_observatory::identity_queue::run_worker(
            identity_queue.clone(),
            identities.clone(),
        ));
    }
    let state = AppState {
        ingester_url,
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
    if runs_workers {
        tokio::spawn(bsky_observatory::ingest::run(
            cfg, storage, analysis, metrics, identities,
        ));
    }
    axum::serve(listener, api::router(state)).await?;
    Ok(())
}
