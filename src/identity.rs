use crate::storage::Storage;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;

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
    pub fn new(
        storage: Storage,
        profile_endpoint: String,
        capacity: usize,
        ttl: Duration,
        negative_ttl: Duration,
        requests_per_second: u32,
    ) -> Self {
        let minimum_request_interval =
            Duration::from_secs_f64(1.0 / requests_per_second.max(1) as f64);
        Self {
            inner: Arc::new(Inner {
                storage,
                client: reqwest::Client::builder()
                    .timeout(Duration::from_secs(8))
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ignores_commits() {
        assert!(
            parse_identity_event(br#"{"kind":"commit"}"#)
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
}
