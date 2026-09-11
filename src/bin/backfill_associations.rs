use anyhow::{Context, Result};
use bsky_observatory::{
    backfill::{retry_delay, valid_backfill_query},
    identity::IdentityRecord,
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
use std::{collections::HashMap, path::PathBuf, time::Duration};
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
#[serde(default)]
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
const GET_POSTS_MAX_URIS: usize = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EngagementPlan {
    likes: bool,
    reposts: bool,
    quotes: bool,
    replies: bool,
}
impl EngagementPlan {
    fn from_post_view(post: &Value) -> Self {
        let positive = |key: &str| post.get(key).and_then(Value::as_u64).unwrap_or(0) > 0;
        Self {
            likes: positive("likeCount"),
            reposts: positive("repostCount"),
            quotes: positive("quoteCount"),
            replies: positive("replyCount"),
        }
    }
    #[cfg(test)]
    fn endpoint_count(self) -> u64 {
        u64::from(self.likes)
            + u64::from(self.reposts)
            + u64::from(self.quotes)
            + u64::from(self.replies)
    }
}

#[cfg(test)]
fn estimated_min_requests(posts: usize, plans: &[EngagementPlan]) -> u64 {
    posts.div_ceil(GET_POSTS_MAX_URIS) as u64
        + plans.iter().map(|plan| plan.endpoint_count()).sum::<u64>()
}

struct Fetcher {
    client: Client,
    pace: tokio::time::Interval,
    requests: u64,
}
#[derive(Default)]
struct IdentityHarvester {
    pending: HashMap<String, IdentityRecord>,
}
impl IdentityHarvester {
    fn observe(&mut self, profile: &Value, expected_did: Option<&str>) -> bool {
        let Some(identity) = identity_from_profile(profile, expected_did) else {
            return false;
        };
        self.pending.insert(identity.did.clone(), identity);
        true
    }
    async fn flush(&mut self, storage: &Storage) -> Result<()> {
        let identities: Vec<_> = self.pending.drain().map(|(_, identity)| identity).collect();
        storage.insert_identities(&identities).await
    }
}
fn identity_from_profile(profile: &Value, expected_did: Option<&str>) -> Option<IdentityRecord> {
    let did = profile.get("did")?.as_str()?;
    let handle = profile.get("handle")?.as_str()?.to_ascii_lowercase();
    if expected_did.is_some_and(|expected| expected != did)
        || !valid_backfill_query(did)
        || !valid_backfill_query(&handle)
        || handle.starts_with("did:")
        || handle == "handle.invalid"
    {
        return None;
    }
    Some(IdentityRecord {
        did: did.into(),
        handle: Some(handle),
        observed_at: Utc::now(),
        source: "association_backfill_appview".into(),
    })
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
            self.requests += 1;
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
        requests: checkpoint.requests,
    };
    let started = std::time::Instant::now();
    let mut identities = IdentityHarvester::default();
    while checkpoint.next_post < posts.len() {
        let start = checkpoint.next_post;
        let end = (start + GET_POSTS_MAX_URIS).min(posts.len());
        let batch = &posts[start..end];
        let query: Vec<_> = batch.iter().map(|uri| ("uris", uri.as_str())).collect();
        let hydrated: Value = match fetcher.json("app.bsky.feed.getPosts", &query).await {
            Ok(value) => value,
            Err(error) => {
                checkpoint.requests = fetcher.requests;
                save_checkpoint(&args.checkpoint, &args.checkpoint_job_id, &checkpoint)?;
                return Err(error).context(format!("hydrate posts {start}..{end}"));
            }
        };
        let views = hydrated["posts"].as_array().cloned().unwrap_or_default();
        for view in &views {
            let expected = view
                .get("uri")
                .and_then(Value::as_str)
                .and_then(|uri| uri.strip_prefix("at://"))
                .and_then(|rest| rest.split('/').next());
            if let Some(author) = view.get("author") {
                identities.observe(author, expected);
            }
        }
        for (index, uri) in posts.iter().enumerate().take(end).skip(start) {
            let view = views
                .iter()
                .find(|post| post.get("uri").and_then(Value::as_str) == Some(uri));
            match view {
                Some(view) => match collect_post(
                    &mut fetcher,
                    uri,
                    &storage,
                    &mut checkpoint,
                    EngagementPlan::from_post_view(view),
                    &mut identities,
                )
                .await
                {
                    Ok(()) => checkpoint.posts_completed += 1,
                    Err(error) => {
                        checkpoint.errors += 1;
                        eprintln!("post={} error={error:#}", uri);
                    }
                },
                None => {
                    checkpoint.errors += 1;
                    eprintln!("post={} missing from getPosts response", uri);
                }
            }
            checkpoint.requests = fetcher.requests;
            checkpoint.next_post = index + 1;
            save_checkpoint(&args.checkpoint, &args.checkpoint_job_id, &checkpoint)?;
        }
        identities.flush(&storage).await?;
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
    plan: EngagementPlan,
    identities: &mut IdentityHarvester,
) -> Result<()> {
    let mut events = Vec::new();
    if plan.likes
        && let Err(error) = collect_likes(fetcher, uri, &mut events, totals, identities).await
    {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=likes error={error:#}");
    }
    if plan.reposts
        && let Err(error) = collect_reposts(fetcher, uri, &mut events, totals, identities).await
    {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=reposts error={error:#}");
    }
    if plan.quotes
        && let Err(error) = collect_quotes(fetcher, uri, &mut events, totals, identities).await
    {
        totals.errors += 1;
        eprintln!("post={uri} endpoint=quotes error={error:#}");
    }
    if plan.replies
        && let Err(error) = collect_replies(fetcher, uri, &mut events, totals, identities).await
    {
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
    identities: &mut IdentityHarvester,
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
            if let Some(actor) = like.get("actor") {
                identities.observe(actor, None);
            }
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
    identities: &mut IdentityHarvester,
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
            identities.observe(actor, None);
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
    identities: &mut IdentityHarvester,
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
            let expected = post
                .get("uri")
                .and_then(Value::as_str)
                .and_then(|uri| uri.strip_prefix("at://"))
                .and_then(|rest| rest.split('/').next());
            if let Some(author) = post.get("author") {
                identities.observe(author, expected);
            }
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
    identities: &mut IdentityHarvester,
) -> Result<()> {
    let page: Value = fetcher
        .json(
            "app.bsky.feed.getPostThread",
            &[("uri", uri), ("depth", "1"), ("parentHeight", "0")],
        )
        .await?;
    let thread = &page["thread"];
    if let Some(author) = thread.pointer("/post/author") {
        identities.observe(
            author,
            Some(
                uri.trim_start_matches("at://")
                    .split('/')
                    .next()
                    .unwrap_or_default(),
            ),
        );
    }
    anyhow::ensure!(
        thread.pointer("/post/uri").and_then(Value::as_str) == Some(uri),
        "thread subject mismatch"
    );
    for reply in thread["replies"].as_array().into_iter().flatten() {
        if let Some(post) = reply.get("post") {
            let expected = post
                .get("uri")
                .and_then(Value::as_str)
                .and_then(|value| value.strip_prefix("at://"))
                .and_then(|rest| rest.split('/').next());
            if let Some(author) = post.get("author") {
                identities.observe(author, expected);
            }
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
    fn legacy_checkpoint_without_posts_total_still_resumes() {
        let checkpoint: Checkpoint = serde_json::from_str(
            r#"{"next_post":1410,"posts_completed":1410,"likes":11309,"reposts":665,"quotes":302,"replies":2209,"requests":5640,"errors":0}"#,
        )
        .unwrap();

        assert_eq!(checkpoint.next_post, 1410);
        assert_eq!(checkpoint.posts_total, 0);
        assert_eq!(checkpoint.likes, 11309);
    }

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
    fn post_view_counts_gate_only_required_endpoints() {
        let empty = EngagementPlan::from_post_view(&serde_json::json!({
            "likeCount":0,"repostCount":0,"quoteCount":0,"replyCount":0
        }));
        assert_eq!(empty.endpoint_count(), 0);
        let mixed = EngagementPlan::from_post_view(&serde_json::json!({
            "likeCount":2,"repostCount":0,"quoteCount":1,"replyCount":0
        }));
        assert_eq!(mixed.endpoint_count(), 2);
        assert!(mixed.likes && mixed.quotes);
        assert!(!mixed.reposts && !mixed.replies);
    }

    #[test]
    fn harvests_known_appview_profile_shapes_without_media() {
        let dids = [
            "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
            "did:plc:bbbbbbbbbbbbbbbbbbbbbbbb",
            "did:plc:cccccccccccccccccccccccc",
            "did:plc:dddddddddddddddddddddddd",
        ];
        let response = serde_json::json!({
            "posts":[{"uri":format!("at://{}/app.bsky.feed.post/a",dids[0]),"author":{"did":dids[0],"handle":"Author.Example.com","avatar":"https://blob.invalid/a"}}],
            "likes":[{"actor":{"did":dids[1],"handle":"liker.example.com","banner":"https://blob.invalid/b"}}],
            "repostedBy":[{"did":dids[2],"handle":"reposter.example.com","avatar":"https://blob.invalid/c"}],
            "thread":{"replies":[{"post":{"uri":format!("at://{}/app.bsky.feed.post/r",dids[3]),"author":{"did":dids[3],"handle":"reply.example.com"}}}]}
        });
        let mut harvested = IdentityHarvester::default();
        let post = &response["posts"][0];
        assert!(harvested.observe(&post["author"], Some(dids[0])));
        assert!(harvested.observe(&response["likes"][0]["actor"], None));
        assert!(harvested.observe(&response["repostedBy"][0], None));
        let reply = &response["thread"]["replies"][0]["post"];
        assert!(harvested.observe(&reply["author"], Some(dids[3])));
        assert_eq!(harvested.pending.len(), 4);
        assert_eq!(
            harvested.pending[dids[0]].handle.as_deref(),
            Some("author.example.com")
        );
        let encoded = serde_json::to_string(&harvested.pending).unwrap();
        assert!(!encoded.contains("blob.invalid"));
        assert!(!encoded.contains("avatar"));
        assert!(!encoded.contains("banner"));
    }

    #[test]
    fn rejects_invalid_or_mismatched_embedded_profiles() {
        let expected = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
        let mut harvested = IdentityHarvester::default();
        assert!(!harvested.observe(&serde_json::json!({"did":"did:plc:bbbbbbbbbbbbbbbbbbbbbbbb","handle":"other.example.com"}),Some(expected)));
        assert!(!harvested.observe(
            &serde_json::json!({"did":expected,"handle":"handle.invalid"}),
            Some(expected)
        ));
        assert!(!harvested.observe(
            &serde_json::json!({"did":"did:web:example.com","handle":"example.com"}),
            None
        ));
        assert!(!harvested.observe(
            &serde_json::json!({"did":expected,"handle":"bad handle.example"}),
            None
        ));
        assert!(harvested.pending.is_empty());
    }

    #[test]
    fn zero_engagement_million_post_estimate_is_one_request_per_batch() {
        let posts = 6_100_000;
        let optimized = estimated_min_requests(posts, &[]);
        let previous = posts as u64 * 4;
        assert_eq!(optimized, 244_000);
        assert_eq!(previous, 24_400_000);
        assert_eq!(previous / optimized, 100);
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
