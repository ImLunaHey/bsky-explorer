use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, net::IpAddr, path::PathBuf, sync::Arc};
use xxhash_rust::xxh3::xxh3_64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    Repository,
    Associations,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Resolving,
    Running,
    Retrying,
    Completed,
    DeadLetter,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillJob {
    pub id: String,
    pub query: String,
    pub did: Option<String>,
    pub handle: Option<String>,
    pub job_type: JobType,
    pub status: JobStatus,
    pub depends_on_job_id: Option<String>,
    pub include_associations: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub collection: Option<String>,
    pub cursor: Option<String>,
    pub records_written: u64,
    pub retry_count: u32,
    pub max_retries: u32,
    pub last_error: Option<String>,
    pub lease_token: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct BackfillQueue {
    root: PathBuf,
    jobs: Arc<Mutex<HashMap<String, BackfillJob>>>,
}

impl BackfillQueue {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let mut jobs = HashMap::new();
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = fs::read(&path)
                && let Ok(job) = serde_json::from_slice::<BackfillJob>(&bytes)
            {
                jobs.insert(job.id.clone(), job);
            }
        }
        Ok(Self {
            root,
            jobs: Arc::new(Mutex::new(jobs)),
        })
    }
    pub fn enqueue(
        &self,
        query: &str,
        did: Option<&str>,
        kind: JobType,
        dependency: Option<String>,
        include_associations: bool,
    ) -> Result<BackfillJob> {
        let scope = match kind {
            JobType::Repository => "repository",
            JobType::Associations => "associations",
        };
        let identity = did.unwrap_or(query).to_ascii_lowercase();
        let id = format!(
            "bf-{:016x}",
            xxh3_64(format!("v1\0{scope}\0{identity}").as_bytes())
        );
        let mut jobs = self.jobs.lock();
        if let Some(job) = jobs.get(&id) {
            return Ok(job.clone());
        }
        let now = Utc::now();
        let job = BackfillJob {
            id: id.clone(),
            query: query.into(),
            did: did.map(str::to_owned),
            handle: None,
            job_type: kind,
            status: JobStatus::Queued,
            depends_on_job_id: dependency,
            include_associations,
            created_at: now,
            updated_at: now,
            next_attempt_at: None,
            collection: None,
            cursor: None,
            records_written: 0,
            retry_count: 0,
            max_retries: 8,
            last_error: None,
            lease_token: None,
            lease_until: None,
        };
        self.persist(&job)?;
        jobs.insert(id, job.clone());
        Ok(job)
    }
    pub fn get(&self, id: &str) -> Option<BackfillJob> {
        self.jobs.lock().get(id).cloned()
    }
    pub fn list(&self, limit: usize) -> Vec<BackfillJob> {
        let mut v: Vec<_> = self.jobs.lock().values().cloned().collect();
        v.sort_by_key(|j| std::cmp::Reverse(j.updated_at));
        v.truncate(limit.min(100));
        v
    }
    pub fn update(&self, job: BackfillJob) -> Result<()> {
        self.persist(&job)?;
        self.jobs.lock().insert(job.id.clone(), job);
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<Option<BackfillJob>> {
        let Some(mut job) = self.get(id) else {
            return Ok(None);
        };
        anyhow::ensure!(
            job.lease_until.is_none_or(|v| v <= Utc::now()),
            "running jobs cannot be cancelled until their lease expires"
        );
        job.status = JobStatus::Cancelled;
        job.updated_at = Utc::now();
        self.update(job.clone())?;
        Ok(Some(job))
    }
    pub fn retry(&self, id: &str) -> Result<Option<BackfillJob>> {
        let Some(mut job) = self.get(id) else {
            return Ok(None);
        };
        anyhow::ensure!(
            matches!(job.status, JobStatus::DeadLetter | JobStatus::Cancelled),
            "only terminal failed jobs can be retried"
        );
        job.status = JobStatus::Queued;
        job.retry_count = 0;
        job.next_attempt_at = None;
        job.last_error = None;
        job.updated_at = Utc::now();
        self.update(job.clone())?;
        Ok(Some(job))
    }
    pub fn claim(&self, lease_seconds: i64) -> Result<Option<BackfillJob>> {
        let now = Utc::now();
        let jobs = self.jobs.lock();
        let candidate = jobs
            .values()
            .find(|j| {
                matches!(
                    j.status,
                    JobStatus::Queued | JobStatus::Retrying | JobStatus::Running
                ) && j.next_attempt_at.is_none_or(|t| t <= now)
                    && j.lease_until.is_none_or(|t| t <= now)
                    && j.depends_on_job_id.as_ref().is_none_or(|id| {
                        jobs.get(id)
                            .is_some_and(|d| d.status == JobStatus::Completed)
                    })
            })
            .cloned();
        drop(jobs);
        if let Some(mut job) = candidate {
            job.status = JobStatus::Running;
            job.updated_at = now;
            job.lease_token = Some(format!(
                "{:016x}",
                xxh3_64(format!("{}{}", job.id, now).as_bytes())
            ));
            job.lease_until = Some(now + Duration::seconds(lease_seconds));
            self.update(job.clone())?;
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }
    fn persist(&self, job: &BackfillJob) -> Result<()> {
        let path = self.root.join(format!("{}.json", job.id));
        let tmp = self.root.join(format!("{}.tmp", job.id));
        fs::write(&tmp, serde_json::to_vec_pretty(job)?)?;
        fs::rename(tmp, path).context("atomically persist backfill job")
    }
}

pub fn retry_delay(headers: &reqwest::header::HeaderMap, now: DateTime<Utc>) -> Option<Duration> {
    if let Some(raw) = headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(seconds) = raw.parse::<i64>() {
            return Some(Duration::seconds(seconds.max(0)));
        }
        if let Ok(date) = DateTime::parse_from_rfc2822(raw) {
            return Some((date.with_timezone(&Utc) - now).max(Duration::zero()));
        }
    }
    let remaining = headers
        .get("ratelimit-remaining")
        .and_then(|v| v.to_str().ok())?
        .parse::<u64>()
        .ok()?;
    if remaining > 0 {
        return None;
    }
    let raw = headers
        .get("ratelimit-reset")?
        .to_str()
        .ok()?
        .parse::<i64>()
        .ok()?;
    let seconds = if raw > now.timestamp() {
        raw - now.timestamp()
    } else {
        raw
    };
    Some(Duration::seconds(seconds.max(0)))
}

pub fn valid_job_id(id: &str) -> bool {
    id.len() == 19 && id.starts_with("bf-") && id[3..].bytes().all(|b| b.is_ascii_hexdigit())
}

pub async fn run_worker(queue: BackfillQueue) {
    loop {
        match queue.claim(120) {
            Ok(Some(job)) => {
                if let Err(error) = execute_job(&queue, job.clone()).await {
                    let mut failed = job;
                    failed.retry_count += 1;
                    failed.last_error = Some(error.to_string().chars().take(1000).collect());
                    failed.updated_at = Utc::now();
                    failed.lease_token = None;
                    failed.lease_until = None;
                    if failed.retry_count >= failed.max_retries {
                        failed.status = JobStatus::DeadLetter;
                    } else {
                        failed.status = JobStatus::Retrying;
                        let jitter = (xxh3_64(failed.id.as_bytes()) % 30) as i64;
                        failed.next_attempt_at = Some(
                            Utc::now()
                                + Duration::seconds((1_i64 << failed.retry_count.min(10)) + jitter),
                        );
                    }
                    let _ = queue.update(failed);
                }
            }
            Ok(None) => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
            Err(error) => {
                tracing::error!(%error, "backfill queue failure");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn execute_job(queue: &BackfillQueue, mut job: BackfillJob) -> Result<()> {
    let did = match job.did.clone() {
        Some(did) => did,
        None => resolve_query(&job.query).await?,
    };
    job.did = Some(did.clone());
    job.updated_at = Utc::now();
    queue.update(job.clone())?;
    let status = match job.job_type {
        JobType::Repository => {
            let pds = resolve_pds(&did).await?;
            tokio::process::Command::new("/usr/local/bin/backfill_account")
                .args(["--repo", &did, "--did", &did, "--pds", &pds])
                .status()
                .await?
        }
        JobType::Associations => {
            let checkpoint = queue.root.join(format!("{}-associations.json", job.id));
            tokio::process::Command::new("/usr/local/bin/backfill_associations")
                .args([
                    "--did",
                    &did,
                    "--checkpoint",
                    checkpoint.to_str().context("invalid checkpoint path")?,
                ])
                .status()
                .await?
        }
    };
    anyhow::ensure!(status.success(), "backfill subprocess failed: {status}");
    job.status = JobStatus::Completed;
    job.updated_at = Utc::now();
    job.lease_token = None;
    job.lease_until = None;
    queue.update(job.clone())?;
    if job.job_type == JobType::Repository && job.include_associations {
        queue.enqueue(&did, Some(&did), JobType::Associations, Some(job.id), false)?;
    }
    Ok(())
}

async fn resolve_query(query: &str) -> Result<String> {
    if query.starts_with("did:") {
        return Ok(query.into());
    }
    let value: serde_json::Value = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(8))
        .build()?
        .get("https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile")
        .query(&[("actor", query)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    value
        .get("did")
        .and_then(|v| v.as_str())
        .filter(|d| d.starts_with("did:"))
        .map(str::to_owned)
        .context("profile returned no DID")
}
async fn resolve_pds(did: &str) -> Result<String> {
    anyhow::ensure!(
        did.starts_with("did:plc:"),
        "only did:plc PDS resolution is supported"
    );
    let endpoint = format!("https://plc.directory/{did}");
    let value: serde_json::Value = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(8))
        .build()?
        .get(endpoint)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let raw = value["service"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|s| s.get("type").and_then(|v| v.as_str()) == Some("AtprotoPersonalDataServer"))
        .and_then(|s| s.get("serviceEndpoint"))
        .and_then(|v| v.as_str())
        .context("DID document has no PDS")?;
    let url = reqwest::Url::parse(raw)?;
    anyhow::ensure!(
        url.scheme() == "https" && url.username().is_empty() && url.password().is_none(),
        "unsafe PDS URL"
    );
    let host = url.host_str().context("PDS URL has no host")?;
    anyhow::ensure!(
        host != "localhost" && !host.ends_with(".local"),
        "unsafe PDS host"
    );
    let addresses =
        tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(443))).await?;
    for address in addresses {
        anyhow::ensure!(
            public_ip(address.ip()),
            "PDS resolves to a private or reserved address"
        );
    }
    Ok(raw.trim_end_matches('/').into())
}
fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified())
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deduplicates_and_resumes() {
        let d = tempfile::tempdir().unwrap();
        let q = BackfillQueue::open(d.path().into()).unwrap();
        let a = q
            .enqueue(
                "did:plc:a",
                Some("did:plc:a"),
                JobType::Repository,
                None,
                true,
            )
            .unwrap();
        let b = q
            .enqueue(
                "did:plc:a",
                Some("did:plc:a"),
                JobType::Repository,
                None,
                true,
            )
            .unwrap();
        assert_eq!(a.id, b.id);
        drop(q);
        assert_eq!(
            BackfillQueue::open(d.path().into())
                .unwrap()
                .get(&a.id)
                .unwrap()
                .id,
            a.id
        );
    }
    #[test]
    fn parses_relative_and_absolute_limits() {
        let now = Utc::now();
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, "12".parse().unwrap());
        assert_eq!(retry_delay(&h, now), Some(Duration::seconds(12)));
        h.remove(reqwest::header::RETRY_AFTER);
        h.insert("ratelimit-remaining", "0".parse().unwrap());
        h.insert(
            "ratelimit-reset",
            (now.timestamp() + 20).to_string().parse().unwrap(),
        );
        assert_eq!(retry_delay(&h, now), Some(Duration::seconds(20)));
    }
    #[test]
    fn validates_ids() {
        assert!(valid_job_id("bf-0123456789abcdef"));
        assert!(!valid_job_id("../bad"));
    }
}
