use clap::Parser;
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone, Parser)]
#[command(version, about)]
pub struct Config {
    #[arg(
        long,
        env = "JETSTREAM_URL",
        default_value = "wss://jetstream2.us-east.bsky.network/subscribe"
    )]
    pub jetstream_url: String,
    #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://localhost:8123")]
    pub clickhouse_url: String,
    #[arg(long, env = "CLICKHOUSE_DATABASE", default_value = "observatory")]
    pub clickhouse_database: String,
    #[arg(long, env = "CLICKHOUSE_USER", default_value = "default")]
    pub clickhouse_user: String,
    #[arg(long, env = "CLICKHOUSE_PASSWORD", default_value = "")]
    pub clickhouse_password: String,
    #[arg(
        long,
        env = "DATA_DIR",
        default_value = "/mnt/storage/bsky-observatory/archive"
    )]
    pub data_dir: PathBuf,
    #[arg(long, env = "BACKFILL_QUEUE_DIR")]
    pub backfill_queue_dir: Option<PathBuf>,
    #[arg(long, env = "BACKFILL_WORKER_CONCURRENCY", default_value_t = 1)]
    pub backfill_worker_concurrency: usize,
    /// Logical allocation for this observatory. `STORAGE_BUDGET_BYTES`, when
    /// set, takes precedence over this TiB value.
    #[arg(long, env = "STORAGE_BUDGET_TIB", default_value_t = 25.0)]
    pub storage_budget_tib: f64,
    #[arg(long, env = "STORAGE_BUDGET_BYTES", default_value_t = 0)]
    pub storage_budget_bytes: u64,
    #[arg(
        long,
        env = "WEB_DIST_DIR",
        default_value = "/usr/share/bsky-observatory"
    )]
    pub web_dist_dir: PathBuf,
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:3000")]
    pub listen_addr: String,
    #[arg(long, env = "ARCHIVE_SEGMENT_MIB", default_value_t = 256)]
    pub archive_segment_mib: u64,
    /// Raw archive retention in days. Zero retains compressed segments forever.
    #[arg(long, env = "ARCHIVE_RETENTION_DAYS", default_value_t = 0)]
    pub archive_retention_days: u64,
    #[arg(long, env = "CLICKHOUSE_BATCH_SIZE", default_value_t = 1000)]
    pub batch_size: usize,
    #[arg(long, env = "CLICKHOUSE_FLUSH_SECONDS", default_value_t = 2)]
    pub flush_seconds: u64,
    #[arg(
        long,
        env = "WINDOWS_SECONDS",
        value_delimiter = ',',
        default_value = "60,300,3600,86400"
    )]
    pub windows_seconds: Vec<u64>,
    #[arg(long, env = "COORDINATION_WINDOW_SECONDS", default_value_t = 30)]
    pub coordination_window_seconds: i64,
    #[arg(long, env = "MIN_CLUSTER_ACCOUNTS", default_value_t = 3)]
    pub min_cluster_accounts: usize,
    #[arg(long, env = "MIN_CLUSTER_EVENTS", default_value_t = 3)]
    pub min_cluster_events: usize,
    #[arg(long, env = "STATE_RETENTION_HOURS", default_value_t = 24)]
    pub state_retention_hours: i64,
    #[arg(
        long,
        env = "IDENTITY_PROFILE_ENDPOINT",
        default_value = "https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile"
    )]
    pub identity_profile_endpoint: String,
    #[arg(long, env = "IDENTITY_CACHE_CAPACITY", default_value_t = 100_000)]
    pub identity_cache_capacity: usize,
    #[arg(long, env = "IDENTITY_CACHE_TTL_SECONDS", default_value_t = 21600)]
    pub identity_cache_ttl_seconds: u64,
    #[arg(long, env = "IDENTITY_NEGATIVE_TTL_SECONDS", default_value_t = 300)]
    pub identity_negative_ttl_seconds: u64,
    #[arg(long, env = "IDENTITY_REQUESTS_PER_SECOND", default_value_t = 2)]
    pub identity_requests_per_second: u32,
    #[arg(long, env = "IDENTITY_QUEUE_DIR")]
    pub identity_queue_dir: Option<PathBuf>,
    #[arg(long, env = "IDENTITY_QUEUE_CAPACITY", default_value_t = 100_000)]
    pub identity_queue_capacity: usize,
    #[arg(long, env = "IDENTITY_WORKER_CONCURRENCY", default_value_t = 1)]
    pub identity_worker_concurrency: usize,
}

impl Config {
    pub fn flush_interval(&self) -> Duration {
        Duration::from_secs(self.flush_seconds)
    }
    pub fn effective_storage_budget_bytes(&self) -> u64 {
        if self.storage_budget_bytes > 0 {
            self.storage_budget_bytes
        } else {
            (self.storage_budget_tib.max(0.0) * 1024_f64.powi(4)).min(u64::MAX as f64) as u64
        }
    }
}
