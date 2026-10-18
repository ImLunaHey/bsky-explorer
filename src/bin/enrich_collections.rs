//! Independent, restartable materializer. Never opens ingestion cursors/queues.
use anyhow::Result;
use bsky_observatory::{config::Config, storage::Storage};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cfg = Config::parse();
    let storage = Storage::new(
        &cfg.clickhouse_url,
        &cfg.clickhouse_database,
        &cfg.clickhouse_user,
        &cfg.clickhouse_password,
    );
    storage.ensure_community_schema().await?;
    loop {
        let result = storage.refresh_music().await;
        let discovery = storage.refresh_discovery().await;
        if std::env::var("ENRICHMENT_ONCE").as_deref() == Ok("1") {
            result?;
            return discovery;
        }
        match result.and(discovery) {
            Ok(()) => {
                std::fs::write(
                    "/tmp/enrichment-ready",
                    chrono::Utc::now().timestamp().to_string(),
                )?;
                tracing::info!("Music and discovery refreshed");
            }
            Err(error) => {
                tracing::warn!(%error, "Music enrichment failed; retaining last good charts")
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
    }
}
