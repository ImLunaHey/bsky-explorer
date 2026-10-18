//! Independent derived-data worker. No ingestion cursor, no remote lookups.
use anyhow::Result;
use bsky_observatory::{
    config::Config,
    storage::{
        Storage,
        follow_resolution::{Cursor, RepairState},
    },
};
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
    let mut state: RepairState = storage
        .snapshot("follow-resolution:cursor")
        .await?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let days = storage.follow_seed_days().await?;
    let mut ticks = 0u64;
    let mut totals = (0, 0);
    loop {
        let result:Result<usize>=async {
            if state.live.time_us==0 {state.live=Cursor{time_us:chrono::Utc::now().timestamp_micros()-600_000_000,event_id:String::new()};}
            let (cursor,live)=storage.repair_follow_batch(&state.live).await?;state.live=cursor;
            // Seed newest retained days first; the MVs capture every new insert.
            if let Some(day)=days.iter().find(|day|!state.seeded.contains(*day)) {
                storage.seed_follow_day(day).await?;state.seeded.insert(day.clone());
                tracing::info!(%day,"Indexed retained follow records");
            }
            let (cursor,history)=storage.repair_follow_batch(&state.history).await?;state.history=cursor;
            storage.save_snapshot("follow-resolution:cursor",&serde_json::to_string(&state)?).await?;
            if ticks.is_multiple_of(30){totals=storage.follow_resolution_totals().await?;}
            storage.save_snapshot("follow-resolution:status",&serde_json::json!({"updated_at":chrono::Utc::now().timestamp(),"seed_remaining_days":days.len()-state.seeded.len().min(days.len()),"indexed_deletions":totals.0,"attributed":totals.1,"unattributed":totals.0.saturating_sub(totals.1),"losses_complete":false,"status":"running"}).to_string()).await?;
            Ok(live+history)
        }.await;
        match result {
            Ok(repaired) => {
                std::fs::write(
                    "/tmp/follow-ready",
                    chrono::Utc::now().timestamp().to_string(),
                )?;
                if repaired > 0 {
                    tracing::info!(repaired, "Attributed unfollows");
                }
            }
            Err(error) => {
                tracing::warn!(%error,"Follow attribution will retry; original events unchanged")
            }
        }
        ticks += 1;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}
