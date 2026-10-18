use bsky_observatory::config::Config;
use clap::Parser;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    bsky_observatory::identity_server::run(Config::parse()).await
}
