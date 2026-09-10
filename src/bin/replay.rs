use anyhow::{Context, Result};
use bsky_observatory::{
    model::NormalizedEvent,
    replay::{ReplayBounds, visit_archive},
    storage::Storage,
};
use clap::Parser;
use std::{path::PathBuf, sync::mpsc};

#[derive(Debug, Parser)]
#[command(about = "Validate or replay compressed archive segments into ClickHouse")]
struct Args {
    #[arg(long, env = "DATA_DIR")]
    archive_dir: PathBuf,
    #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://localhost:8123")]
    clickhouse_url: String,
    #[arg(long, env = "CLICKHOUSE_DATABASE", default_value = "observatory")]
    clickhouse_database: String,
    #[arg(long, env = "CLICKHOUSE_USER", default_value = "default")]
    clickhouse_user: String,
    #[arg(long, env = "CLICKHOUSE_PASSWORD", default_value = "")]
    clickhouse_password: String,
    #[arg(long, default_value_t = 1000)]
    batch_size: usize,
    #[arg(long)]
    since_time_us: Option<i64>,
    #[arg(long)]
    until_time_us: Option<i64>,
    /// Actually insert rows. Without this flag the command only validates and counts.
    #[arg(long)]
    write: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    anyhow::ensure!(args.batch_size > 0, "batch-size must be greater than zero");
    let bounds = ReplayBounds {
        since_time_us: args.since_time_us,
        until_time_us: args.until_time_us,
    };
    let (sender, receiver) = mpsc::sync_channel::<NormalizedEvent>(args.batch_size * 2);
    let archive_dir = args.archive_dir.clone();
    let reader = tokio::task::spawn_blocking(move || {
        visit_archive(&archive_dir, bounds, |event| {
            sender.send(event).context("replay consumer stopped")
        })
    });
    let storage = Storage::new(
        &args.clickhouse_url,
        &args.clickhouse_database,
        &args.clickhouse_user,
        &args.clickhouse_password,
    );
    let mut batch = Vec::with_capacity(args.batch_size);
    while let Ok(event) = receiver.recv() {
        batch.push(event);
        if batch.len() == args.batch_size {
            if args.write {
                storage.insert_events(&batch).await?;
            }
            batch.clear();
        }
    }
    if args.write {
        storage.insert_events(&batch).await?;
    }
    let stats = reader.await.context("archive reader panicked")??;
    println!(
        "mode={} raw={} normalized={} unsupported={} outside_time_range={}",
        if args.write { "write" } else { "validate" },
        stats.raw_events,
        stats.normalized_events,
        stats.unsupported_events,
        stats.outside_time_range,
    );
    Ok(())
}
