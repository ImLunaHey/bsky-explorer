use anyhow::Result;
use bsky_observatory::{
    analysis::Analyzer,
    api::{self, AppState, GrowthBaseline},
    archive::Archive,
    config::Config,
    metrics::Metrics,
    storage::Storage,
};
use clap::Parser;
use tracing_subscriber::EnvFilter;
#[tokio::main]
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
    );
    let state = AppState {
        analysis: analysis.clone(),
        metrics: metrics.clone(),
        storage: storage.clone(),
        archive_dir: cfg.data_dir.clone(),
        growth_baseline: GrowthBaseline {
            measured_at: std::time::Instant::now(),
            bytes: Archive::total_size(&cfg.data_dir) + storage.bytes().await.unwrap_or(0),
        },
    };
    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr).await?;
    tokio::spawn(bsky_observatory::ingest::run(
        cfg, storage, analysis, metrics,
    ));
    axum::serve(listener, api::router(state)).await?;
    Ok(())
}
