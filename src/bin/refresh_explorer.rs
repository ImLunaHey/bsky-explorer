//! Prewarm persisted summaries before switching a deployment to live traffic.
use anyhow::Result;
use bsky_observatory::{
    config::Config,
    storage::{Storage, overview::Window},
};
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config::parse();
    let storage = Storage::new(
        &cfg.clickhouse_url,
        &cfg.clickhouse_database,
        &cfg.clickhouse_user,
        &cfg.clickhouse_password,
    );
    storage.ensure_community_schema().await?;
    for window in [Window::Hour, Window::Day, Window::Week] {
        let started = std::time::Instant::now();
        let data = storage
            .overview(window, chrono::Utc::now().timestamp())
            .await?;
        storage
            .save_snapshot(
                &format!("atproto-overview:{}", window.seconds()),
                &serde_json::to_string(&data)?,
            )
            .await?;
        println!(
            "Prepared {}s overview in {:.1}s",
            window.seconds(),
            started.elapsed().as_secs_f64()
        );
    }
    let data = storage.coverage().await?;
    storage
        .save_snapshot(
            "coverage",
            &serde_json::json!({"generated_at":chrono::Utc::now().timestamp(),"data":data})
                .to_string(),
        )
        .await?;
    println!("Prepared dataset summary");
    Ok(())
}
