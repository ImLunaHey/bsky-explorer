use anyhow::{Context, Result};
use bsky_observatory::{
    backfill::retry_delay,
    model::{EventKind, NormalizedEvent},
    normalize::normalize_appview_post,
    storage::Storage,
};
use chrono::{DateTime, Utc};
use clap::Parser;
use reqwest::{Client, StatusCode, redirect::Policy};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{path::PathBuf, time::Duration};
use tokio::time::{MissedTickBehavior, interval};
use xxhash_rust::xxh3::xxh3_64;

const APPVIEW: &str = "https://public.api.bsky.app/xrpc/";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Parser)]
#[command(about = "Checkpointed AppView relationship backfill for one retained account's posts")]
struct Args {
    #[arg(long)]
    did: String,
    #[arg(long)]
    checkpoint: PathBuf,
    #[arg(long)]
    checkpoint_job_id: String,
    #[arg(long, default_value_t = 4)]
    requests_per_second: u32,
    #[arg(long, env = "CLICKHOUSE_URL", default_value = "http://localhost:8123")]
    clickhouse_url: String,
    #[arg(long, env = "CLICKHOUSE_DATABASE", default_value = "observatory")]
    clickhouse_database: String,
    #[arg(long, env = "CLICKHOUSE_USER", default_value = "default")]
    clickhouse_user: String,
    #[arg(long, env = "CLICKHOUSE_PASSWORD", default_value = "")]
    clickhouse_password: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Checkpoint {
    next_post: usize,
    posts_total: u64,
    posts_completed: u64,
    likes: u64,
    reposts: u64,
    quotes: u64,
    replies: u64,
    requests: u64,
    errors: u64,
}

struct Fetcher {
    client: Client,
    pace: tokio::time::Interval,
}

impl Fetcher {
    async fn json<T: DeserializeOwned>(
        &mut self,
        method: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let endpoint = format!("{APPVIEW}{method}");
        let mut delay = Duration::from_secs(1);
        for attempt in 0..5 {
            self.pace.tick().await;
            let response = self.client.get(&endpoint).query(query).send().await;
            match response {
                Ok(response) if response.status().is_success() => {
                    anyhow::ensure!(
                        response
                            .content_length()
                            .is_none_or(|n| n <= MAX_RESPONSE_BYTES as u64),
                        "AppView response too large"
                    );
                    let bytes = response.bytes().await?;
                    anyhow::ensure!(
                        bytes.len() <= MAX_RESPONSE_BYTES,
                        "AppView response too large"
                    );
                    return serde_json::from_slice(&bytes).context("decode AppView response");
                }
                Ok(response)
                    if response.status() == StatusCode::TOO_MANY_REQUESTS
                        || response.status().is_server_error() =>
                {
                    if let Some(wait) = retry_delay(response.headers(), Utc::now())
                        .and_then(|value| value.to_std().ok())
                    {
                        delay = wait.min(Duration::from_secs(300));
                    }
                }
                Ok(response) => anyhow::bail!("AppView returned {}", response.status()),
                Err(error) if attempt == 4 => return Err(error.into()),
                Err(_) => {}
            }
            let jitter = Duration::from_millis(xxh3_64(endpoint.as_bytes()) % 500);
            tokio::time::sleep((delay + jitter).min(Duration::from_secs(300))).await;
            delay = (delay * 2).min(Duration::from_secs(16));
        }
        anyhow::bail!("AppView retry limit reached")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    anyhow::ensure!(args.did.starts_with("did:"), "--did must be a DID");
    anyhow::ensure!(
        args.requests_per_second > 0 && args.requests_per_second <= 20,
        "rate must be 1-20 requests/sec"
    );
    let storage = Storage::new(
        &args.clickhouse_url,
        &args.clickhouse_database,
        &args.clickhouse_user,
        &args.clickhouse_password,
    );
    storage.ping().await?;
    let posts = storage.account_post_uris(&args.did).await?;
    let mut checkpoint = load_checkpoint(&args.checkpoint, &args.checkpoint_job_id)?;
    anyhow::ensure!(
        checkpoint.next_post <= posts.len(),
        "checkpoint exceeds current post list"
    );
    checkpoint.posts_total = posts.len() as u64;
    save_checkpoint(&args.checkpoint, &args.checkpoint_job_id, &checkpoint)?;
    let mut pace = interval(Duration::from_millis(
        1000 / u64::from(args.requests_per_second),
    ));
    pace.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut fetcher = Fetcher {
        client: Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(12))
            .redirect(Policy::none())
            .user_agent("bsky-observatory-association-backfill/0.1")
            .build()?,
        pace,
    };
    let started = std::time::Instant::now();
    for (index, uri) in posts.iter().enumerate().skip(checkpoint.next_post) {
        let before = checkpoint.requests;
        match collect_post(&mut fetcher, uri, &storage, &mut checkpoint).await {
            Ok(()) => checkpoint.posts_completed += 1,
            Err(error) => {
                checkpoint.errors += 1;
                eprintln!("post={} error={error:#}", uri);
            }
        }
        checkpoint.requests += 4.max(checkpoint.requests.saturating_sub(before));
        checkpoint.next_post = index + 1;
        save_checkpoint(&args.checkpoint, &args.checkpoint_job_id, &checkpoint)?;
        if checkpoint.posts_completed % 100 == 0 {
            let rate = checkpoint.posts_completed as f64 / started.elapsed().as_secs_f64();
            let remaining = posts.len().saturating_sub(checkpoint.next_post);
            eprintln!(
                "completed={}/{} associations={} errors={} eta_seconds={:.0}",
                checkpoint.next_post,
                posts.len(),
                checkpoint.likes + checkpoint.reposts + checkpoint.quotes + checkpoint.replies,
                checkpoint.errors,
                remaining as f64 / rate.max(0.001)
            );
        }
    }
    println!("{}", serde_json::to_string_pretty(&checkpoint)?);
    Ok(())
}

async fn collect_post(
    fetcher: &mut Fetcher,
    uri: &str,
    storage: &Storage,
    totals: &mut Checkpoint,
) -> Result<()> {
    let mut events = Vec::new();
    if let Err(error) = collect_likes(fetcher, uri, &mut events, totals).await {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=likes error={error:#}");
    }
    if let Err(error) = collect_reposts(fetcher, uri, &mut events, totals).await {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=reposts error={error:#}");
    }
    if let Err(error) = collect_quotes(fetcher, uri, &mut events, totals).await {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=quotes error={error:#}");
    }
    if let Err(error) = collect_replies(fetcher, uri, &mut events, totals).await {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=thread error={error:#}");
    }
    let ids: Vec<_> = events.iter().map(|event| event.event_id.clone()).collect();
    let existing = storage.existing_event_ids(&ids).await?;
    events.retain(|event| !existing.contains(&event.event_id));
    storage.insert_events(&events).await
}

async fn collect_likes(
    fetcher: &mut Fetcher,
    uri: &str,
    out: &mut Vec<NormalizedEvent>,
    totals: &mut Checkpoint,
) -> Result<()> {
    let mut cursor = None;
    loop {
        let mut query = vec![("uri", uri), ("limit", "100")];
        if let Some(value) = cursor.as_deref() {
            query.push(("cursor", value));
        }
        let page: Value = fetcher.json("app.bsky.feed.getLikes", &query).await?;
        ensure_subject(&page, uri)?;
        for like in page["likes"].as_array().into_iter().flatten() {
            let actor = like
                .pointer("/actor/did")
                .and_then(Value::as_str)
                .context("like actor missing")?;
            let timestamp = parse_time(like.get("createdAt").or_else(|| like.get("indexedAt")));
            out.push(association(EventKind::Like, actor, uri, timestamp));
            totals.likes += 1;
        }
        cursor = page
            .get("cursor")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
        if cursor.is_none() {
            break;
        }
    }
    Ok(())
}

async fn collect_reposts(
    fetcher: &mut Fetcher,
    uri: &str,
    out: &mut Vec<NormalizedEvent>,
    totals: &mut Checkpoint,
) -> Result<()> {
    let mut cursor = None;
    loop {
        let mut query = vec![("uri", uri), ("limit", "100")];
        if let Some(value) = cursor.as_deref() {
            query.push(("cursor", value));
        }
        let page: Value = fetcher.json("app.bsky.feed.getRepostedBy", &query).await?;
        ensure_subject(&page, uri)?;
        for actor in page["repostedBy"].as_array().into_iter().flatten() {
            let did = actor
                .get("did")
                .and_then(Value::as_str)
                .context("reposter DID missing")?;
            out.push(association(EventKind::Repost, did, uri, Utc::now()));
            totals.reposts += 1;
        }
        cursor = page
            .get("cursor")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
        if cursor.is_none() {
            break;
        }
    }
    Ok(())
}

async fn collect_quotes(
    fetcher: &mut Fetcher,
    uri: &str,
    out: &mut Vec<NormalizedEvent>,
    totals: &mut Checkpoint,
) -> Result<()> {
    let mut cursor = None;
    loop {
        let mut query = vec![("uri", uri), ("limit", "100")];
        if let Some(value) = cursor.as_deref() {
            query.push(("cursor", value));
        }
        let page: Value = fetcher.json("app.bsky.feed.getQuotes", &query).await?;
        ensure_subject(&page, uri)?;
        for post in page["posts"].as_array().into_iter().flatten() {
            let mut event = normalize_appview_post(post, Utc::now())?;
            apply_record_time(&mut event, post);
            event.kind = EventKind::Quote;
            event.operation = "association_backfill".into();
            event.target = Some(uri.into());
            event.quote_target = Some(uri.into());
            event.event_id = deterministic_id("quote", &event.actor_did, uri, Some(&event.at_uri));
            out.push(event);
            totals.quotes += 1;
        }
        cursor = page
            .get("cursor")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .filter(|s| !s.is_empty());
        if cursor.is_none() {
            break;
        }
    }
    Ok(())
}

async fn collect_replies(
    fetcher: &mut Fetcher,
    uri: &str,
    out: &mut Vec<NormalizedEvent>,
    totals: &mut Checkpoint,
) -> Result<()> {
    let page: Value = fetcher
        .json(
            "app.bsky.feed.getPostThread",
            &[("uri", uri), ("depth", "1"), ("parentHeight", "0")],
        )
        .await?;
    let thread = &page["thread"];
    anyhow::ensure!(
        thread.pointer("/post/uri").and_then(Value::as_str) == Some(uri),
        "thread subject mismatch"
    );
    for reply in thread["replies"].as_array().into_iter().flatten() {
        if let Some(post) = reply.get("post") {
            let mut event = normalize_appview_post(post, Utc::now())?;
            apply_record_time(&mut event, post);
            event.kind = EventKind::Reply;
            event.operation = "association_backfill".into();
            event.target = Some(uri.into());
            event.reply_parent = Some(uri.into());
            event.event_id = deterministic_id("reply", &event.actor_did, uri, Some(&event.at_uri));
            out.push(event);
            totals.replies += 1;
        }
    }
    Ok(())
}

fn association(
    kind: EventKind,
    actor: &str,
    target: &str,
    timestamp: DateTime<Utc>,
) -> NormalizedEvent {
    let name = kind.as_str();
    NormalizedEvent {
        event_id: deterministic_id(name, actor, target, None),
        time_us: timestamp.timestamp_micros(),
        timestamp,
        actor_did: actor.into(),
        kind,
        collection: format!("app.bsky.feed.{name}"),
        rkey: String::new(),
        operation: "association_backfill".into(),
        at_uri: String::new(),
        record_json: None,
        target: Some(target.into()),
        target_did: target
            .strip_prefix("at://")
            .and_then(|s| s.split('/').next())
            .map(str::to_owned),
        text: None,
        reply_parent: None,
        reply_root: None,
        quote_target: None,
        urls: vec![],
        domains: vec![],
        languages: vec![],
        text_hash: None,
        normalized_text_hash: None,
        simhash: None,
    }
}

fn deterministic_id(kind: &str, actor: &str, target: &str, record: Option<&str>) -> String {
    let key = format!("{kind}\0{actor}\0{target}\0{}", record.unwrap_or(""));
    format!("association_backfill:{:016x}", xxh3_64(key.as_bytes()))
}
fn parse_time(value: Option<&Value>) -> DateTime<Utc> {
    value
        .and_then(Value::as_str)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}
fn apply_record_time(event: &mut NormalizedEvent, post: &Value) {
    let timestamp = parse_time(
        post.pointer("/record/createdAt")
            .or_else(|| post.get("indexedAt")),
    );
    event.timestamp = timestamp;
    event.time_us = timestamp.timestamp_micros();
}
fn ensure_subject(value: &Value, uri: &str) -> Result<()> {
    anyhow::ensure!(
        value.get("uri").and_then(Value::as_str) == Some(uri),
        "AppView subject mismatch"
    );
    Ok(())
}
fn load_checkpoint(path: &PathBuf, job_id: &str) -> Result<Checkpoint> {
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
fn save_checkpoint(path: &PathBuf, job_id: &str, value: &Checkpoint) -> Result<()> {
    let db = rusqlite::Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute("INSERT INTO backfill_checkpoints(job_id,kind,payload,updated_at) VALUES(?1,'associations',?2,?3) ON CONFLICT(job_id) DO UPDATE SET payload=excluded.payload,updated_at=excluded.updated_at",rusqlite::params![job_id,serde_json::to_string(value)?,Utc::now().to_rfc3339()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_deterministic_and_type_specific() {
        assert_eq!(
            deterministic_id("like", "did:a", "at://p", None),
            deterministic_id("like", "did:a", "at://p", None)
        );
        assert_ne!(
            deterministic_id("like", "did:a", "at://p", None),
            deterministic_id("repost", "did:a", "at://p", None)
        );
    }
    #[test]
    fn association_has_no_blob_or_record_payload() {
        let event = association(
            EventKind::Like,
            "did:plc:a",
            "at://did:plc:b/app.bsky.feed.post/x",
            Utc::now(),
        );
        assert_eq!(event.operation, "association_backfill");
        assert!(event.record_json.is_none());
        assert!(event.text.is_none());
    }
    #[test]
    fn imported_text_helpers_remain_stable() {
        assert_eq!(bsky_observatory::normalize::normalize_text("Hi"), "hi");
        assert_eq!(
            bsky_observatory::normalize::simhash("same"),
            bsky_observatory::normalize::simhash("same")
        );
    }
}
