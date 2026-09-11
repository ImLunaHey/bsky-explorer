use crate::storage::Storage;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug)]
pub enum RemoteIdentityResult {
    Resolved(IdentityRecord),
    Negative,
    RetryAfter(Duration, String),
}
#[derive(Debug)]
pub enum RemoteIdentityBatchResult {
    Resolved(HashMap<String, IdentityRecord>),
    RetryAfter(Duration, String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityRecord {
    pub did: String,
    pub handle: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub source: String,
}

/// Parse Jetstream's identity envelope. These events are independent of repo
/// commits and are emitted when a DID's validated handle changes.
pub fn parse_identity_event(raw: &[u8]) -> Result<Option<IdentityRecord>> {
    let value: Value = serde_json::from_slice(raw).context("invalid Jetstream JSON")?;
    if value.get("kind").and_then(Value::as_str) != Some("identity") {
        return Ok(None);
    }
    let identity = value.get("identity").unwrap_or(&Value::Null);
    let did = identity
        .get("did")
        .and_then(Value::as_str)
        .or_else(|| value.get("did").and_then(Value::as_str))
        .unwrap_or_default();
    if did.is_empty() {
        anyhow::bail!("identity event has no DID");
    }
    let handle = identity
        .get("handle")
        .and_then(Value::as_str)
        .filter(|handle| !handle.is_empty() && *handle != "handle.invalid")
        .map(str::to_owned);
    let observed_at = value
        .get("time_us")
        .and_then(Value::as_i64)
        .and_then(DateTime::from_timestamp_micros)
        .unwrap_or_else(Utc::now);
    Ok(Some(IdentityRecord {
        did: did.to_owned(),
        handle,
        observed_at,
        source: "jetstream".into(),
    }))
}

#[derive(Clone)]
pub struct IdentityService {
    inner: Arc<Inner>,
}

struct Inner {
    storage: Storage,
    client: reqwest::Client,
    profile_endpoint: String,
    ttl: Duration,
    negative_ttl: Duration,
    capacity: usize,
    minimum_request_interval: Duration,
    cache: Mutex<Cache>,
    last_request: AsyncMutex<Option<Instant>>,
}

#[derive(Default)]
struct Cache {
    values: HashMap<String, CacheEntry>,
    order: VecDeque<String>,
}

#[derive(Clone)]
struct CacheEntry {
    identity: IdentityRecord,
    expires_at: Instant,
}

impl IdentityService {
    pub async fn unresolved_candidates(&self, limit: u64) -> Result<Vec<String>> {
        self.inner.storage.unresolved_identity_dids(limit).await
    }
    pub fn new(
        storage: Storage,
        profile_endpoint: String,
        capacity: usize,
        ttl: Duration,
        negative_ttl: Duration,
        requests_per_second: u32,
    ) -> Self {
        let profile_endpoint =
            if profile_endpoint == "https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile" {
                profile_endpoint
            } else {
                tracing::warn!(endpoint=%profile_endpoint, "rejecting non-fixed identity endpoint");
                "https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile".into()
            };
        let minimum_request_interval =
            Duration::from_secs_f64(1.0 / requests_per_second.max(1) as f64);
        Self {
            inner: Arc::new(Inner {
                storage,
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(8))
                    .redirect(reqwest::redirect::Policy::none())
                    .user_agent("bsky-observatory/0.1")
                    .build()
                    .expect("valid HTTP client"),
                profile_endpoint,
                ttl,
                negative_ttl,
                capacity: capacity.max(1),
                minimum_request_interval,
                cache: Mutex::new(Cache::default()),
                last_request: AsyncMutex::new(None),
            }),
        }
    }

    pub async fn observe(&self, identity: IdentityRecord) -> Result<()> {
        self.put_cache(identity.clone(), self.inner.ttl);
        self.inner.storage.insert_identity(&identity).await
    }

    pub async fn resolve(&self, did: &str) -> Result<IdentityRecord> {
        if !did.starts_with("did:") {
            anyhow::bail!("invalid DID");
        }
        if let Some(identity) = self.cached(did) {
            return Ok(identity);
        }
        if let Some(identity) = self.inner.storage.identity(did).await? {
            let age = Utc::now()
                .signed_duration_since(identity.observed_at)
                .to_std()
                .unwrap_or(Duration::ZERO);
            if age < self.inner.ttl {
                self.put_cache(identity.clone(), self.inner.ttl - age);
                return Ok(identity);
            }
        }
        let identity = self
            .resolve_public_profile(did)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(%error, %did, "DID profile resolution failed");
                IdentityRecord {
                    did: did.to_owned(),
                    handle: None,
                    observed_at: Utc::now(),
                    source: "unresolved".into(),
                }
            });
        let duration = if identity.handle.is_some() {
            self.inner.ttl
        } else {
            self.inner.negative_ttl
        };
        self.put_cache(identity.clone(), duration);
        // Persist successful lookups. Negative cache entries are intentionally
        // process-local so a transient public API outage does not become history.
        if identity.handle.is_some() {
            self.inner.storage.insert_identity(&identity).await?;
        }
        Ok(identity)
    }

    /// Resolve only from the hot cache or persisted firehose observations.
    /// Request paths that return activity data must not wait on the public
    /// network merely to decorate a DID with a handle.
    pub async fn resolve_local(&self, did: &str) -> Result<IdentityRecord> {
        if !did.starts_with("did:") {
            anyhow::bail!("invalid DID");
        }
        if let Some(identity) = self.cached(did) {
            return Ok(identity);
        }
        Ok(self
            .inner
            .storage
            .identity(did)
            .await?
            .unwrap_or_else(|| IdentityRecord {
                did: did.to_owned(),
                handle: None,
                observed_at: Utc::now(),
                source: "unresolved".into(),
            }))
    }

    async fn resolve_public_profile(&self, did: &str) -> Result<IdentityRecord> {
        let mut last_request = self.inner.last_request.lock().await;
        if let Some(previous) = *last_request {
            tokio::time::sleep(
                self.inner
                    .minimum_request_interval
                    .saturating_sub(previous.elapsed()),
            )
            .await;
        }
        *last_request = Some(Instant::now());
        drop(last_request);

        let response = self
            .inner
            .client
            .get(&self.inner.profile_endpoint)
            .query(&[("actor", did)])
            .send()
            .await?
            .error_for_status()?;
        let value: Value = response.json().await?;
        let returned_did = value
            .get("did")
            .and_then(Value::as_str)
            .context("profile has no DID")?;
        if returned_did != did {
            anyhow::bail!("profile DID does not match request");
        }
        let handle = value
            .get("handle")
            .and_then(Value::as_str)
            .filter(|handle| !handle.is_empty() && *handle != "handle.invalid")
            .map(str::to_owned);
        Ok(IdentityRecord {
            did: did.to_owned(),
            handle,
            observed_at: Utc::now(),
            source: "public_profile".into(),
        })
    }

    /// Perform exactly one rate-limited network attempt for the durable worker.
    pub async fn resolve_remote(&self, did: &str) -> Result<RemoteIdentityResult> {
        anyhow::ensure!(did.starts_with("did:"), "invalid DID");
        let mut last_request = self.inner.last_request.lock().await;
        if let Some(previous) = *last_request {
            tokio::time::sleep(
                self.inner
                    .minimum_request_interval
                    .saturating_sub(previous.elapsed()),
            )
            .await;
        }
        *last_request = Some(Instant::now());
        drop(last_request);
        let response = self
            .inner
            .client
            .get(&self.inner.profile_endpoint)
            .query(&[("actor", did)])
            .send()
            .await?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        {
            let delay = crate::backfill::retry_delay(response.headers(), Utc::now())
                .unwrap_or_else(|| chrono::Duration::seconds(30))
                .to_std()
                .unwrap_or(Duration::from_secs(30))
                .min(Duration::from_secs(3600));
            return Ok(RemoteIdentityResult::RetryAfter(
                delay,
                format!("profile service returned {status}"),
            ));
        }
        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::BAD_REQUEST {
            return Ok(RemoteIdentityResult::Negative);
        }
        let response = response.error_for_status()?;
        anyhow::ensure!(
            response.content_length().is_none_or(|n| n <= 262_144),
            "profile response too large"
        );
        let body = response.bytes().await?;
        anyhow::ensure!(body.len() <= 262_144, "profile response too large");
        let value: Value = serde_json::from_slice(&body)?;
        anyhow::ensure!(
            value.get("did").and_then(Value::as_str) == Some(did),
            "profile DID does not match request"
        );
        let handle = value
            .get("handle")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && *v != "handle.invalid")
            .map(str::to_owned);
        let identity = IdentityRecord {
            did: did.to_owned(),
            handle,
            observed_at: Utc::now(),
            source: "identity_worker".into(),
        };
        if identity.handle.is_none() {
            return Ok(RemoteIdentityResult::Negative);
        }
        self.observe(identity.clone()).await?;
        Ok(RemoteIdentityResult::Resolved(identity))
    }

    /// Resolve up to the AppView limit in one paced request. Missing requested
    /// actors are intentional negative results; unexpected or duplicate DIDs
    /// reject the whole response so no job is incorrectly completed.
    pub async fn resolve_remote_batch(&self, dids: &[String]) -> Result<RemoteIdentityBatchResult> {
        anyhow::ensure!(
            !dids.is_empty() && dids.len() <= 25,
            "identity batch must contain 1..=25 DIDs"
        );
        let requested: HashSet<&str> = dids.iter().map(String::as_str).collect();
        anyhow::ensure!(
            requested.len() == dids.len() && requested.iter().all(|v| v.starts_with("did:")),
            "invalid identity batch"
        );
        let mut last = self.inner.last_request.lock().await;
        if let Some(previous) = *last {
            tokio::time::sleep(
                self.inner
                    .minimum_request_interval
                    .saturating_sub(previous.elapsed()),
            )
            .await
        }
        *last = Some(Instant::now());
        drop(last);
        let endpoint = self
            .inner
            .profile_endpoint
            .replace("app.bsky.actor.getProfile", "app.bsky.actor.getProfiles");
        let response = self
            .inner
            .client
            .get(endpoint)
            .query(&dids.iter().map(|v| ("actors", v)).collect::<Vec<_>>())
            .send()
            .await?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status.is_server_error()
        {
            let delay = crate::backfill::retry_delay(response.headers(), Utc::now())
                .unwrap_or_else(|| chrono::Duration::seconds(30))
                .to_std()
                .unwrap_or(Duration::from_secs(30))
                .min(Duration::from_secs(3600));
            return Ok(RemoteIdentityBatchResult::RetryAfter(
                delay,
                format!("profile service returned {status}"),
            ));
        }
        let response = response.error_for_status()?;
        anyhow::ensure!(
            response.content_length().is_none_or(|n| n <= 1_048_576),
            "profile batch response too large"
        );
        let body = response.bytes().await?;
        anyhow::ensure!(body.len() <= 1_048_576, "profile batch response too large");
        let value: Value = serde_json::from_slice(&body)?;
        let found = parse_batch_profiles(&value, &requested)?;
        let identities = found.values().cloned().collect::<Vec<_>>();
        for identity in &identities {
            self.put_cache(identity.clone(), self.inner.ttl);
        }
        self.inner.storage.insert_identities(&identities).await?;
        Ok(RemoteIdentityBatchResult::Resolved(found))
    }

    fn cached(&self, did: &str) -> Option<IdentityRecord> {
        let mut cache = self.inner.cache.lock();
        match cache.values.get(did) {
            Some(entry) if entry.expires_at > Instant::now() => Some(entry.identity.clone()),
            Some(_) => {
                cache.values.remove(did);
                None
            }
            None => None,
        }
    }

    fn put_cache(&self, identity: IdentityRecord, ttl: Duration) {
        let mut cache = self.inner.cache.lock();
        if cache
            .values
            .get(&identity.did)
            .is_some_and(|current| current.identity.observed_at > identity.observed_at)
        {
            return;
        }
        cache.order.retain(|did| did != &identity.did);
        cache.order.push_back(identity.did.clone());
        cache.values.insert(
            identity.did.clone(),
            CacheEntry {
                identity,
                expires_at: Instant::now() + ttl,
            },
        );
        while cache.values.len() > self.inner.capacity {
            if let Some(oldest) = cache.order.pop_front() {
                cache.values.remove(&oldest);
            }
        }
    }
}

fn parse_batch_profiles(
    value: &Value,
    requested: &HashSet<&str>,
) -> Result<HashMap<String, IdentityRecord>> {
    let profiles = value
        .get("profiles")
        .and_then(Value::as_array)
        .context("profile batch has no profiles array")?;
    let mut found = HashMap::new();
    for profile in profiles {
        let did = profile
            .get("did")
            .and_then(Value::as_str)
            .context("batch profile has no DID")?;
        anyhow::ensure!(
            requested.contains(did),
            "profile batch returned unrequested DID"
        );
        anyhow::ensure!(
            !found.contains_key(did),
            "profile batch returned duplicate DID"
        );
        let handle = profile
            .get("handle")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && *v != "handle.invalid")
            .map(str::to_owned);
        if handle.is_some() {
            found.insert(
                did.into(),
                IdentityRecord {
                    did: did.into(),
                    handle,
                    observed_at: Utc::now(),
                    source: "identity_worker_batch".into(),
                },
            );
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_service(endpoint: String) -> IdentityService {
        IdentityService {
            inner: Arc::new(Inner {
                storage: Storage::new("http://localhost:8123", "observatory", "default", ""),
                client: reqwest::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                profile_endpoint: endpoint,
                ttl: Duration::from_secs(60),
                negative_ttl: Duration::from_secs(1),
                capacity: 100,
                minimum_request_interval: Duration::ZERO,
                cache: Mutex::new(Cache::default()),
                last_request: AsyncMutex::new(None),
            }),
        }
    }
    fn mock_once(response: &'static str) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0_u8; 16384];
            let n = stream.read(&mut bytes).unwrap();
            let _ = tx.send(String::from_utf8_lossy(&bytes[..n]).into_owned());
            stream.write_all(response.as_bytes()).unwrap()
        });
        (
            format!("http://{address}/xrpc/app.bsky.actor.getProfile"),
            rx,
        )
    }

    #[test]
    fn parses_handle_change_and_invalid_handle() {
        let changed = br#"{"did":"did:plc:alice","time_us":1700000000000000,"kind":"identity","identity":{"did":"did:plc:alice","handle":"alice.example"}}"#;
        let parsed = parse_identity_event(changed).unwrap().unwrap();
        assert_eq!(parsed.did, "did:plc:alice");
        assert_eq!(parsed.handle.as_deref(), Some("alice.example"));

        let invalid = br#"{"did":"did:plc:alice","time_us":1700000000000001,"kind":"identity","identity":{"handle":"handle.invalid"}}"#;
        assert_eq!(parse_identity_event(invalid).unwrap().unwrap().handle, None);
    }

    #[test]
    fn normalized_firehose_commits_do_not_create_identity_updates() {
        assert!(
            parse_identity_event(br#"{"kind":"commit","did":"did:plc:actor","commit":{"collection":"app.bsky.feed.like","record":{"subject":{"uri":"at://did:plc:target/app.bsky.feed.post/x"}}}}"#)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn cache_is_bounded_and_updates_handles() {
        let service = IdentityService::new(
            Storage::new("http://localhost:8123", "observatory", "default", ""),
            "http://unused.invalid".into(),
            2,
            Duration::from_secs(60),
            Duration::from_secs(1),
            10,
        );
        for (did, handle) in [("did:plc:a", "a.test"), ("did:plc:b", "b.test")] {
            service.put_cache(
                IdentityRecord {
                    did: did.into(),
                    handle: Some(handle.into()),
                    observed_at: Utc::now(),
                    source: "test".into(),
                },
                Duration::from_secs(60),
            );
        }
        service.put_cache(
            IdentityRecord {
                did: "did:plc:b".into(),
                handle: Some("renamed.test".into()),
                observed_at: Utc::now(),
                source: "jetstream".into(),
            },
            Duration::from_secs(60),
        );
        service.put_cache(
            IdentityRecord {
                did: "did:plc:c".into(),
                handle: Some("c.test".into()),
                observed_at: Utc::now(),
                source: "test".into(),
            },
            Duration::from_secs(60),
        );
        assert!(service.cached("did:plc:a").is_none());
        assert_eq!(
            service.cached("did:plc:b").unwrap().handle.as_deref(),
            Some("renamed.test")
        );
        assert!(service.cached("did:plc:c").is_some());
    }

    #[test]
    fn parses_full_and_partial_profile_batches_safely() {
        let dids = (0..25)
            .map(|n| format!("did:plc:test{n}"))
            .collect::<Vec<_>>();
        let requested = dids.iter().map(String::as_str).collect::<HashSet<_>>();
        let profiles = dids
            .iter()
            .map(|did| serde_json::json!({"did":did,"handle":format!("{did}.test")}))
            .collect::<Vec<_>>();
        let found =
            parse_batch_profiles(&serde_json::json!({"profiles":profiles}), &requested).unwrap();
        assert_eq!(found.len(), 25);
        let partial = parse_batch_profiles(
            &serde_json::json!({"profiles":[{"did":dids[0],"handle":"one.test"}]}),
            &requested,
        )
        .unwrap();
        assert_eq!(partial.len(), 1);
        assert!(
            parse_batch_profiles(
                &serde_json::json!({"profiles":[{"did":"did:plc:unexpected","handle":"bad.test"}]}),
                &requested
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn batches_twenty_five_actors_in_one_request() {
        let (endpoint, request) = mock_once(
            "HTTP/1.1 200 OK\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"profiles\":[]}",
        );
        let dids = (0..25)
            .map(|n| format!("did:plc:test{n}"))
            .collect::<Vec<_>>();
        let result = test_service(endpoint)
            .resolve_remote_batch(&dids)
            .await
            .unwrap();
        assert!(matches!(result,RemoteIdentityBatchResult::Resolved(found) if found.is_empty()));
        let request = request.recv().unwrap();
        assert_eq!(request.matches("actors=").count(), 25);
        assert!(request.starts_with("GET /xrpc/app.bsky.actor.getProfiles?"));
    }
    #[tokio::test]
    async fn batch_retry_after_and_response_limits_are_enforced() {
        let (endpoint, _) = mock_once(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 7\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let result = test_service(endpoint)
            .resolve_remote_batch(&["did:plc:a".into()])
            .await
            .unwrap();
        assert!(
            matches!(result,RemoteIdentityBatchResult::RetryAfter(d,_) if d==Duration::from_secs(7))
        );
        let (endpoint, _) =
            mock_once("HTTP/1.1 200 OK\r\nContent-Length: 2000000\r\nConnection: close\r\n\r\n");
        assert!(
            test_service(endpoint)
                .resolve_remote_batch(&["did:plc:a".into()])
                .await
                .unwrap_err()
                .to_string()
                .contains("too large")
        );
    }
    #[tokio::test]
    async fn batch_client_does_not_follow_redirects() {
        let (endpoint, _) = mock_once(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        assert!(
            test_service(endpoint)
                .resolve_remote_batch(&["did:plc:a".into()])
                .await
                .is_err()
        );
    }
}
