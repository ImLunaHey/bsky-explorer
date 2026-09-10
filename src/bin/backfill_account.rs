use anyhow::{Context, Result};
use bsky_observatory::{normalize::normalize_repository_record, storage::Storage};
use chrono::Utc;
use clap::Parser;
use reqwest::{Client, Url, redirect::Policy};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Parser)]
#[command(about = "Backfill one public ATProto repository into normalized ClickHouse storage")]
struct Args {
    #[arg(long)]
    repo: String,
    #[arg(long)]
    did: String,
    #[arg(long)]
    pds: String,
    #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://localhost:8123")]
    clickhouse_url: String,
    #[arg(long, env = "CLICKHOUSE_DATABASE", default_value = "observatory")]
    clickhouse_database: String,
    #[arg(long, env = "CLICKHOUSE_USER", default_value = "default")]
    clickhouse_user: String,
    #[arg(long, env = "CLICKHOUSE_PASSWORD", default_value = "")]
    clickhouse_password: String,
}

#[derive(Deserialize)]
struct RepoDescription {
    did: String,
    collections: Vec<String>,
}
#[derive(Deserialize)]
struct RecordPage {
    records: Vec<RepositoryRecord>,
    cursor: Option<String>,
}
#[derive(Deserialize)]
struct RepositoryRecord {
    uri: String,
    value: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    anyhow::ensure!(args.did.starts_with("did:"), "--did must be a DID");
    let pds = Url::parse(&args.pds).context("invalid --pds URL")?;
    anyhow::ensure!(pds.scheme() == "https", "--pds must use HTTPS");
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(20))
        .redirect(Policy::none())
        .user_agent("bsky-observatory-backfill/0.1")
        .build()?;
    let storage = Storage::new(
        &args.clickhouse_url,
        &args.clickhouse_database,
        &args.clickhouse_user,
        &args.clickhouse_password,
    );
    storage.ping().await?;
    let description: RepoDescription = get_json(
        &client,
        pds.join("/xrpc/com.atproto.repo.describeRepo")?.as_str(),
        &[("repo", args.repo.as_str())],
    )
    .await?;
    anyhow::ensure!(description.did == args.did, "PDS returned a different DID");

    let mut counts = BTreeMap::new();
    for collection in description.collections {
        let mut cursor: Option<String> = None;
        let mut inserted = 0_u64;
        loop {
            let mut query = vec![
                ("repo", args.repo.as_str()),
                ("collection", collection.as_str()),
                ("limit", "100"),
            ];
            if let Some(value) = cursor.as_deref() {
                query.push(("cursor", value));
            }
            let page: RecordPage = get_json(
                &client,
                pds.join("/xrpc/com.atproto.repo.listRecords")?.as_str(),
                &query,
            )
            .await
            .with_context(|| format!("list {collection}"))?;
            let uris: Vec<String> = page
                .records
                .iter()
                .map(|record| record.uri.clone())
                .collect();
            let existing = storage.existing_uris(&uris).await?;
            let events: Vec<_> = page
                .records
                .iter()
                .filter(|record| !existing.contains(&record.uri))
                .map(|record| {
                    normalize_repository_record(
                        &record.uri,
                        &args.did,
                        &record.value,
                        "pds_backfill",
                        Utc::now(),
                    )
                })
                .collect::<Result<_>>()?;
            storage.insert_events(&events).await?;
            inserted += events.len() as u64;
            cursor = page.cursor.filter(|value| !value.is_empty());
            if cursor.is_none() {
                break;
            }
        }
        counts.insert(collection, inserted);
    }
    println!("{}", serde_json::to_string_pretty(&counts)?);
    Ok(())
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    client: &Client,
    endpoint: &str,
    query: &[(&str, &str)],
) -> Result<T> {
    let response = client
        .get(endpoint)
        .query(query)
        .send()
        .await?
        .error_for_status()?;
    anyhow::ensure!(
        response
            .content_length()
            .is_none_or(|length| length <= MAX_RESPONSE_BYTES as u64),
        "PDS response exceeds size limit"
    );
    let bytes = response.bytes().await?;
    anyhow::ensure!(
        bytes.len() <= MAX_RESPONSE_BYTES,
        "PDS response exceeds size limit"
    );
    serde_json::from_slice(&bytes).context("decode PDS response")
}
