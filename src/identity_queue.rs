use crate::identity::{IdentityService, RemoteIdentityResult};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityJobStatus {
    Queued,
    Running,
    Retrying,
    Resolved,
    Negative,
    DeadLetter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityJob {
    pub id: String,
    pub did: String,
    pub status: IdentityJobStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub last_error: Option<String>,
    pub lease_token: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct IdentityQueue {
    root: PathBuf,
    jobs: Arc<Mutex<HashMap<String, IdentityJob>>>,
    capacity: usize,
    negative_ttl: Duration,
}

impl IdentityQueue {
    pub fn open(root: PathBuf, capacity: usize, negative_ttl: Duration) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let mut jobs = HashMap::new();
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) == Some("json")
                && let Ok(v) = fs::read(&path)
                    .and_then(|b| serde_json::from_slice(&b).map_err(std::io::Error::other))
            {
                let j: IdentityJob = v;
                jobs.insert(j.did.clone(), j);
            }
        }
        Ok(Self {
            root,
            jobs: Arc::new(Mutex::new(jobs)),
            capacity: capacity.max(100),
            negative_ttl,
        })
    }
    pub fn enqueue(&self, did: &str) -> Result<IdentityJob> {
        anyhow::ensure!(valid_did(did), "invalid DID");
        let now = Utc::now();
        let mut jobs = self.jobs.lock();
        if let Some(j) = jobs.get(did)
            && (j.status != IdentityJobStatus::Negative || now - j.updated_at < self.negative_ttl)
        {
            return Ok(j.clone());
        }
        if jobs.len() >= self.capacity {
            anyhow::bail!("identity queue capacity reached; retained history was not pruned");
        }
        let id = format!("id-{:016x}", xxh3_64(did.as_bytes()));
        let job = IdentityJob {
            id: id.clone(),
            did: did.into(),
            status: IdentityJobStatus::Queued,
            created_at: now,
            updated_at: now,
            next_attempt_at: None,
            attempts: 0,
            max_attempts: 8,
            last_error: None,
            lease_token: None,
            lease_until: None,
        };
        self.persist(&job)?;
        jobs.insert(did.into(), job.clone());
        Ok(job)
    }
    pub fn list(&self, limit: usize) -> Vec<IdentityJob> {
        let mut v: Vec<_> = self.jobs.lock().values().cloned().collect();
        v.sort_by_key(|j| std::cmp::Reverse(j.updated_at));
        v.truncate(limit.min(100));
        v
    }
    pub fn summary(&self) -> IdentityQueueSummary {
        let jobs = self.jobs.lock();
        let mut s = IdentityQueueSummary {
            total: jobs.len(),
            queued: 0,
            running: 0,
            retrying: 0,
            resolved: 0,
            negative: 0,
            dead_letter: 0,
        };
        for j in jobs.values() {
            match j.status {
                IdentityJobStatus::Queued => s.queued += 1,
                IdentityJobStatus::Running => s.running += 1,
                IdentityJobStatus::Retrying => s.retrying += 1,
                IdentityJobStatus::Resolved => s.resolved += 1,
                IdentityJobStatus::Negative => s.negative += 1,
                IdentityJobStatus::DeadLetter => s.dead_letter += 1,
            }
        }
        s
    }
    fn persist(&self, j: &IdentityJob) -> Result<()> {
        let p = self.root.join(format!("{}.json", j.id));
        let t = self.root.join(format!("{}.tmp", j.id));
        fs::write(&t, serde_json::to_vec(j)?)?;
        fs::rename(t, p).context("persist identity job")
    }
    fn claim(&self) -> Result<Option<IdentityJob>> {
        let now = Utc::now();
        let mut jobs = self.jobs.lock();
        let Some(key) = jobs
            .iter()
            .find(|(_, j)| {
                matches!(
                    j.status,
                    IdentityJobStatus::Queued | IdentityJobStatus::Retrying
                ) && (j.next_attempt_at.is_none_or(|v| v <= now))
                    && (j.lease_until.is_none_or(|v| v <= now))
            })
            .map(|(k, _)| k.clone())
        else {
            return Ok(None);
        };
        let j = jobs.get_mut(&key).unwrap();
        j.status = IdentityJobStatus::Running;
        j.attempts += 1;
        j.updated_at = now;
        j.lease_token = Some(format!(
            "{:016x}",
            xxh3_64(format!("{}{}", j.id, now).as_bytes())
        ));
        j.lease_until = Some(now + Duration::minutes(2));
        let out = j.clone();
        self.persist(&out)?;
        Ok(Some(out))
    }
    fn finish(
        &self,
        mut j: IdentityJob,
        status: IdentityJobStatus,
        error: Option<String>,
        delay: Option<std::time::Duration>,
    ) -> Result<()> {
        j.status = status;
        j.updated_at = Utc::now();
        j.last_error = error;
        j.next_attempt_at = delay
            .and_then(|d| Duration::from_std(d).ok())
            .map(|d| Utc::now() + d);
        j.lease_token = None;
        j.lease_until = None;
        self.persist(&j)?;
        self.jobs.lock().insert(j.did.clone(), j);
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct IdentityQueueSummary {
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub retrying: usize,
    pub resolved: usize,
    pub negative: usize,
    pub dead_letter: usize,
}
pub async fn run_worker(queue: IdentityQueue, service: IdentityService) {
    let mut idle_ticks = 0_u32;
    loop {
        match queue.claim() {
            Ok(Some(j)) => {
                let result = service.resolve_remote(&j.did).await;
                let (status, err, delay) = match result {
                    Ok(RemoteIdentityResult::Resolved(_)) => {
                        (IdentityJobStatus::Resolved, None, None)
                    }
                    Ok(RemoteIdentityResult::Negative) => (IdentityJobStatus::Negative, None, None),
                    Ok(RemoteIdentityResult::RetryAfter(d, e)) => {
                        (IdentityJobStatus::Retrying, Some(e), Some(d))
                    }
                    Err(e) => {
                        let base = (2u64.pow(j.attempts.min(8))).min(300);
                        let jitter = xxh3_64(format!("{}:{}", j.id, j.attempts).as_bytes()) % 1000;
                        let d = std::time::Duration::from_millis(base * 1000 + jitter);
                        if j.attempts >= j.max_attempts {
                            (IdentityJobStatus::DeadLetter, Some(e.to_string()), None)
                        } else {
                            (IdentityJobStatus::Retrying, Some(e.to_string()), Some(d))
                        }
                    }
                };
                if let Err(e) = queue.finish(j, status, err, delay) {
                    tracing::error!(%e,"persist identity job")
                }
            }
            Ok(None) => {
                idle_ticks = idle_ticks.wrapping_add(1);
                if idle_ticks.is_multiple_of(120) {
                    match service.unresolved_candidates(250).await {
                        Ok(dids) => {
                            for did in dids {
                                let _ = queue.enqueue(&did);
                            }
                        }
                        Err(error) => tracing::debug!(%error, "identity discovery sweep failed"),
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await
            }
            Err(e) => {
                tracing::error!(%e,"claim identity job");
                tokio::time::sleep(std::time::Duration::from_secs(2)).await
            }
        }
    }
}
fn valid_did(v: &str) -> bool {
    v.len() <= 256
        && v.starts_with("did:")
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":._-%".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dedupes_and_negative_expires() {
        let p = std::env::temp_dir().join(format!(
            "identity-q-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let q = IdentityQueue::open(p.clone(), 100, Duration::seconds(1)).unwrap();
        let a = q.enqueue("did:plc:abc").unwrap();
        let b = q.enqueue("did:plc:abc").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(q.summary().total, 1);
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn rejects_unsafe_did() {
        let p = std::env::temp_dir().join("identity-q-invalid");
        let q = IdentityQueue::open(p.clone(), 100, Duration::seconds(1)).unwrap();
        assert!(q.enqueue("https://127.0.0.1").is_err());
        let _ = fs::remove_dir_all(p);
    }

    #[test]
    fn capacity_never_prunes_retained_history() {
        let p = std::env::temp_dir().join(format!(
            "identity-q-capacity-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        ));
        let q = IdentityQueue::open(p.clone(), 100, Duration::seconds(1)).unwrap();
        for n in 0..100 {
            q.enqueue(&format!("did:plc:test{n}")).unwrap();
        }
        assert!(q.enqueue("did:plc:overflow").is_err());
        assert_eq!(q.summary().total, 100);
        assert_eq!(fs::read_dir(&p).unwrap().count(), 100);
        fs::remove_dir_all(p).unwrap();
    }
}
