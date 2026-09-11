use anyhow::{Context, Result};
use atrium_repo::{Repository, blockstore::CarStore};
use bsky_observatory::{
    backfill::retry_delay, normalize::normalize_repository_record, storage::Storage,
};
use chrono::Utc;
use clap::Parser;
use futures_util::{StreamExt, TryStreamExt};
use ipld_core::ipld::Ipld;
use reqwest::{Client, Url, redirect::Policy};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_CAR_BYTES: u64 = 64 * 1024 * 1024 * 1024;

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
    #[serde(default)]
    car_record_index: usize,
    #[serde(default)]
    car_complete: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    anyhow::ensure!(args.did.starts_with("did:"), "--did must be a DID");
    let pds = Url::parse(&args.pds).context("invalid --pds URL")?;
    anyhow::ensure!(pds.scheme() == "https", "--pds must use HTTPS");
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(600))
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
    if let (Some(checkpoint_db), Some(job_id)) =
        (args.checkpoint.as_ref(), args.checkpoint_job_id.as_deref())
    {
        let car_dir = checkpoint_db
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("repository-cars");
        tokio::fs::create_dir_all(&car_dir).await?;
        let car_path = car_dir.join(format!("{job_id}.car"));
        if !checkpoint.car_complete {
            download_car(&client, &pds, &args.repo, &car_path).await?;
            process_car(
                &car_path,
                &args.did,
                &storage,
                &mut checkpoint,
                args.checkpoint.as_ref(),
                args.checkpoint_job_id.as_deref(),
            )
            .await?;
            checkpoint.car_complete = true;
            save_checkpoint(
                args.checkpoint.as_ref(),
                args.checkpoint_job_id.as_deref(),
                &checkpoint,
            )?;
        }
        let _ = tokio::fs::remove_file(&car_path).await;
        println!(
            "{}",
            serde_json::json!({"mode":"car","records_written":checkpoint.records_written})
        );
        return Ok(());
    }
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

async fn download_car(client: &Client, pds: &Url, repo: &str, path: &Path) -> Result<()> {
    let endpoint = pds.join("/xrpc/com.atproto.sync.getRepo")?;
    let response = client
        .get(endpoint)
        .timeout(Duration::from_secs(6 * 3600))
        .query(&[("did", repo)])
        .send()
        .await?
        .error_for_status()?;
    if let Some(length) = response.content_length() {
        anyhow::ensure!(length <= MAX_CAR_BYTES, "repository CAR exceeds size limit")
    }
    let partial = path.with_extension("car.part");
    let mut file = tokio::fs::File::create(&partial).await?;
    let mut received = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        received = received.saturating_add(chunk.len() as u64);
        anyhow::ensure!(
            received <= MAX_CAR_BYTES,
            "repository CAR exceeds size limit"
        );
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    tokio::fs::rename(partial, path).await?;
    Ok(())
}

async fn process_car(
    path: &Path,
    did: &str,
    storage: &Storage,
    checkpoint: &mut RepoCheckpoint,
    checkpoint_path: Option<&PathBuf>,
    job_id: Option<&str>,
) -> Result<()> {
    let file = tokio::fs::File::open(path).await?;
    let car = CarStore::open(file).await.context("index repository CAR")?;
    let root = car.roots().next().context("CAR has no repository root")?;
    let mut repo = Repository::open(car, root)
        .await
        .context("open repository CAR")?;
    let entries = repo
        .tree()
        .entries()
        .try_collect::<Vec<_>>()
        .await
        .context("enumerate repository MST")?;
    anyhow::ensure!(
        checkpoint.car_record_index <= entries.len(),
        "CAR checkpoint exceeds repository record count"
    );
    for batch in entries[checkpoint.car_record_index..].chunks(1000) {
        let mut events = Vec::with_capacity(batch.len());
        for (key, _) in batch {
            let Some(record) = repo
                .get_raw::<Ipld>(key)
                .await
                .context("read repository record")?
            else {
                continue;
            };
            let Some(value) = ipld_json(record) else {
                continue;
            };
            let uri = format!("at://{did}/{key}");
            events.push(normalize_repository_record(
                &uri,
                did,
                &value,
                "pds_backfill",
                Utc::now(),
            )?);
        }
        let uris = events
            .iter()
            .map(|event| event.at_uri.clone())
            .collect::<Vec<_>>();
        let existing = storage.existing_uris(&uris).await?;
        events.retain(|event| !existing.contains(&event.at_uri));
        storage.insert_events(&events).await?;
        checkpoint.records_written += events.len() as u64;
        checkpoint.car_record_index += batch.len();
        checkpoint.collection = entries
            .get(checkpoint.car_record_index.saturating_sub(1))
            .and_then(|(key, _)| key.split('/').next())
            .map(str::to_owned);
        checkpoint.cursor = Some(format!("car:{}", checkpoint.car_record_index));
        save_checkpoint(checkpoint_path, job_id, checkpoint)?;
    }
    checkpoint.collection = None;
    checkpoint.cursor = None;
    Ok(())
}

fn ipld_json(value: Ipld) -> Option<Value> {
    Some(match value {
        Ipld::Null => Value::Null,
        Ipld::Bool(v) => Value::Bool(v),
        Ipld::Integer(v) => serde_json::Number::from_i128(v).map(Value::Number)?,
        Ipld::Float(v) => serde_json::Number::from_f64(v).map(Value::Number)?,
        Ipld::String(v) => Value::String(v),
        Ipld::Bytes(_) => return None,
        Ipld::Link(cid) => serde_json::json!({"$link":cid.to_string()}),
        Ipld::List(values) => Value::Array(values.into_iter().filter_map(ipld_json).collect()),
        Ipld::Map(values) => Value::Object(
            values
                .into_iter()
                .filter_map(|(key, value)| ipld_json(value).map(|value| (key, value)))
                .collect(),
        ),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipld_conversion_keeps_links_and_drops_binary_values() {
        let cid = "bafyreihyrpefhacm6kkp4ql6j6udakdit7g3dmkzfriqfykhjw6lgh3moa"
            .parse()
            .unwrap();
        let value = ipld_json(Ipld::Map(BTreeMap::from([
            ("text".into(), Ipld::String("hello".into())),
            ("ref".into(), Ipld::Link(cid)),
            ("bytes".into(), Ipld::Bytes(vec![1, 2, 3])),
        ])))
        .unwrap();
        assert_eq!(value["text"], "hello");
        assert!(value["ref"]["$link"].as_str().unwrap().starts_with("bafy"));
        assert!(value.get("bytes").is_none());
    }
    #[test]
    fn old_repository_checkpoints_default_to_car_not_complete() {
        let checkpoint: RepoCheckpoint = serde_json::from_str(
            r#"{"collection_index":2,"collection":null,"cursor":null,"records_written":9}"#,
        )
        .unwrap();
        assert_eq!(checkpoint.car_record_index, 0);
        assert!(!checkpoint.car_complete);
    }
}
