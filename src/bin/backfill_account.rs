use anyhow::{Context, Result};
use bsky_observatory::{
    backfill::retry_delay, normalize::normalize_repository_record, storage::Storage,
};
use chrono::Utc;
use clap::Parser;
use reqwest::{Client, Url, redirect::Policy};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

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
    #[arg(long)]
    checkpoint: Option<PathBuf>,
    #[arg(long)]
    checkpoint_job_id: Option<String>,
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
#[derive(Default, Serialize, Deserialize)]
struct RepoCheckpoint {
    collection_index: usize,
    collection: Option<String>,
    cursor: Option<String>,
    records_written: u64,
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

    let mut checkpoint =
        load_checkpoint(args.checkpoint.as_ref(), args.checkpoint_job_id.as_deref())?;
    let mut counts = BTreeMap::new();
    for (collection_index, collection) in description
        .collections
        .into_iter()
        .enumerate()
        .skip(checkpoint.collection_index)
    {
        let mut cursor = (checkpoint.collection.as_deref() == Some(&collection))
            .then(|| checkpoint.cursor.clone())
            .flatten();
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
            checkpoint.collection_index = collection_index;
            checkpoint.collection = Some(collection.clone());
            checkpoint.cursor = cursor.clone();
            checkpoint.records_written += events.len() as u64;
            save_checkpoint(
                args.checkpoint.as_ref(),
                args.checkpoint_job_id.as_deref(),
                &checkpoint,
            )?;
            if cursor.is_none() {
                break;
            }
        }
        counts.insert(collection, inserted);
        checkpoint.collection_index = collection_index + 1;
        checkpoint.collection = None;
        checkpoint.cursor = None;
        save_checkpoint(
            args.checkpoint.as_ref(),
            args.checkpoint_job_id.as_deref(),
            &checkpoint,
        )?;
    }
    println!("{}", serde_json::to_string_pretty(&counts)?);
    Ok(())
}

async fn get_json<T: DeserializeOwned>(
    client: &Client,
    endpoint: &str,
    query: &[(&str, &str)],
) -> Result<T> {
    let mut backoff = Duration::from_secs(1);
    let mut response = None;
    for attempt in 0..6 {
        let candidate = client.get(endpoint).query(query).send().await?;
        if candidate.status().is_success() {
            response = Some(candidate);
            break;
        }
        if candidate.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            || candidate.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || candidate.status().is_server_error()
        {
            if attempt == 5 {
                return Err(candidate.error_for_status().unwrap_err().into());
            }
            let indicated = retry_delay(candidate.headers(), chrono::Utc::now())
                .and_then(|v| v.to_std().ok())
                .unwrap_or(backoff);
            let jitter =
                Duration::from_millis(xxhash_rust::xxh3::xxh3_64(endpoint.as_bytes()) % 500);
            tokio::time::sleep((indicated + jitter).min(Duration::from_secs(300))).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
            continue;
        }
        candidate.error_for_status()?;
    }
    let response = response.context("PDS retry limit reached")?;
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
fn load_checkpoint(path: Option<&PathBuf>, job_id: Option<&str>) -> Result<RepoCheckpoint> {
    let (Some(path), Some(job_id)) = (path, job_id) else {
        return Ok(RepoCheckpoint::default());
    };
    let db = rusqlite::Connection::open(path)?;
    let raw = db
        .query_row(
            "SELECT payload FROM backfill_checkpoints WHERE job_id=?1",
            [job_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
        .map(Option::unwrap_or_default)
}
fn save_checkpoint(
    path: Option<&PathBuf>,
    job_id: Option<&str>,
    value: &RepoCheckpoint,
) -> Result<()> {
    let (Some(path), Some(job_id)) = (path, job_id) else {
        return Ok(());
    };
    let db = rusqlite::Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute("INSERT INTO backfill_checkpoints(job_id,kind,payload,updated_at) VALUES(?1,'repository',?2,?3) ON CONFLICT(job_id) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at",rusqlite::params![job_id,serde_json::to_string(value)?,Utc::now().to_rfc3339()])?;
    Ok(())
}
