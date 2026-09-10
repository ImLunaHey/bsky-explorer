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
        anyhow::ensure!(valid_backfill_query(query), "invalid backfill identity");
        if let Some(did) = did {
            anyhow::ensure!(
                did.starts_with("did:plc:") && valid_backfill_query(did),
                "invalid backfill DID"
            );
        }
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
        let active = jobs
            .values()
            .filter(|job| {
                matches!(
                    job.status,
                    JobStatus::Queued
                        | JobStatus::Resolving
                        | JobStatus::Running
                        | JobStatus::Retrying
                )
            })
            .count();
        anyhow::ensure!(
            jobs.len() < 10_000,
            "backfill queue retention limit reached"
        );
        anyhow::ensure!(active < 100, "backfill queue is at its active-job limit");
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
        let mut jobs = self.jobs.lock();
        let blocked: Vec<_> = jobs
            .values()
            .filter(|job| job.status == JobStatus::Queued)
            .filter_map(|job| {
                let dependency = job.depends_on_job_id.as_ref()?;
                jobs.get(dependency)
                    .is_some_and(|parent| {
                        matches!(parent.status, JobStatus::DeadLetter | JobStatus::Cancelled)
                    })
                    .then(|| job.id.clone())
            })
            .collect();
        for id in blocked {
            let job = jobs.get_mut(&id).expect("blocked dependent exists");
            job.status = JobStatus::DeadLetter;
            job.last_error = Some("dependency did not complete".into());
            job.updated_at = now;
            self.persist(job)?;
        }
        let candidate_id = jobs
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
            .map(|job| job.id.clone());
        if let Some(id) = candidate_id {
            let job = jobs.get_mut(&id).expect("selected queue job exists");
            job.status = JobStatus::Running;
            job.updated_at = now;
            job.lease_token = Some(format!(
                "{:016x}",
                xxh3_64(format!("{}{}", job.id, now).as_bytes())
            ));
            job.lease_until = Some(now + Duration::seconds(lease_seconds));
            self.persist(job)?;
            Ok(Some(job.clone()))
        } else {
            Ok(None)
        }
    }
    fn renew_lease(&self, id: &str, token: &str, lease_seconds: i64) -> Result<()> {
        let checkpoint = self.checkpoint_progress(id);
        let mut jobs = self.jobs.lock();
        let job = jobs.get_mut(id).context("leased job disappeared")?;
        anyhow::ensure!(
            job.lease_token.as_deref() == Some(token),
            "backfill lease ownership changed"
        );
        anyhow::ensure!(
            job.status == JobStatus::Running,
            "backfill job is no longer running"
        );
        job.lease_until = Some(Utc::now() + Duration::seconds(lease_seconds));
        job.updated_at = Utc::now();
        if let Some((records, collection, cursor)) = checkpoint {
            job.records_written = records;
            job.collection = collection;
            job.cursor = cursor;
        }
        self.persist(job)
    }
    fn association_progress(&self, id: &str) -> Option<u64> {
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(self.root.join(format!("{id}-associations.json"))).ok()?,
        )
        .ok()?;
        Some(
            ["likes", "reposts", "quotes", "replies"]
                .iter()
                .filter_map(|key| value.get(key)?.as_u64())
                .sum(),
        )
    }
    fn checkpoint_progress(&self, id: &str) -> Option<(u64, Option<String>, Option<String>)> {
        let repository = self.root.join(format!("{id}-repository.json"));
        if let Ok(bytes) = fs::read(repository) {
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            return Some((
                value
                    .get("records_written")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                value
                    .get("collection")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                value
                    .get("cursor")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
            ));
        }
        self.association_progress(id)
            .map(|records| (records, None, None))
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

pub fn valid_backfill_query(query: &str) -> bool {
    if let Some(suffix) = query.strip_prefix("did:plc:") {
        return suffix.len() == 24
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit());
    }
    if query.len() > 253 || !query.contains('.') || query.starts_with('.') || query.ends_with('.') {
        return false;
    }
    query.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
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
    let mut command = match job.job_type {
        JobType::Repository => {
            let pds = resolve_pds(&did).await?;
            let checkpoint = queue.root.join(format!("{}-repository.json", job.id));
            let mut command = tokio::process::Command::new("/usr/local/bin/backfill_account");
            command.args([
                "--repo",
                &did,
                "--did",
                &did,
                "--pds",
                &pds,
                "--checkpoint",
                checkpoint.to_str().context("invalid checkpoint path")?,
            ]);
            command
        }
        JobType::Associations => {
            let checkpoint = queue.root.join(format!("{}-associations.json", job.id));
            let mut command = tokio::process::Command::new("/usr/local/bin/backfill_associations");
            command.args([
                "--did",
                &did,
                "--checkpoint",
                checkpoint.to_str().context("invalid checkpoint path")?,
            ]);
            command
        }
    };
    command.kill_on_drop(true);
    let mut child = command.spawn()?;
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    let token = job
        .lease_token
        .clone()
        .context("running job has no lease token")?;
    let status = loop {
        tokio::select! {
            status=child.wait()=>break status?,
            _=heartbeat.tick()=>queue.renew_lease(&job.id,&token,120)?,
        }
    };
    anyhow::ensure!(status.success(), "backfill subprocess failed: {status}");
    if let Some((records, collection, cursor)) = queue.checkpoint_progress(&job.id) {
        job.records_written = records;
        job.collection = collection;
        job.cursor = cursor;
    }
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
        url.port_or_known_default() == Some(443),
        "PDS must use HTTPS port 443"
    );
    anyhow::ensure!(
        url.host()
            .is_some_and(|host| matches!(host, url::Host::Domain(_))),
        "PDS endpoint must use a DNS hostname"
    );
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
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240)
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast()
                || ip.segments()[0] == 0x2001 && ip.segments()[1] == 0x0db8
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|mapped| !public_ip(IpAddr::V4(mapped))))
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
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                JobType::Repository,
                None,
                true,
            )
            .unwrap();
        let b = q
            .enqueue(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
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
    #[test]
    fn validates_only_plc_dids_and_dns_style_handles_for_queueing() {
        assert!(valid_backfill_query("did:plc:k6acu4chiwkixvdedcmdgmal"));
        assert!(valid_backfill_query("lunafications.blue"));
        for invalid in [
            "ordinary words.with spaces",
            "localhost",
            ".example.com",
            "example.com.",
            "-bad.example",
            "did:web:example.com",
            "did:plc:short",
            "https://example.com",
        ] {
            assert!(!valid_backfill_query(invalid), "accepted {invalid}");
        }
    }
    #[test]
    fn a_job_cannot_be_claimed_twice_during_its_lease() {
        let directory = tempfile::tempdir().unwrap();
        let queue = BackfillQueue::open(directory.path().into()).unwrap();
        queue
            .enqueue(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                JobType::Repository,
                None,
                false,
            )
            .unwrap();
        let claimed = queue.claim(120).unwrap().unwrap();
        assert!(claimed.lease_token.is_some());
        assert!(queue.claim(120).unwrap().is_none());
    }

    #[test]
    fn terminal_dependency_does_not_leave_child_queued_forever() {
        let directory = tempfile::tempdir().unwrap();
        let queue = BackfillQueue::open(directory.path().into()).unwrap();
        let parent = queue
            .enqueue(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                JobType::Repository,
                None,
                false,
            )
            .unwrap();
        let child = queue
            .enqueue(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                JobType::Associations,
                Some(parent.id.clone()),
                false,
            )
            .unwrap();
        let mut failed = parent;
        failed.status = JobStatus::DeadLetter;
        queue.update(failed).unwrap();
        assert!(queue.claim(120).unwrap().is_none());
        let child = queue.get(&child.id).unwrap();
        assert_eq!(child.status, JobStatus::DeadLetter);
        assert_eq!(
            child.last_error.as_deref(),
            Some("dependency did not complete")
        );
    }

    #[test]
    fn association_checkpoint_progress_is_reflected_in_durable_job() {
        let directory = tempfile::tempdir().unwrap();
        let queue = BackfillQueue::open(directory.path().into()).unwrap();
        let job = queue
            .enqueue(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                JobType::Associations,
                None,
                false,
            )
            .unwrap();
        fs::write(
            directory
                .path()
                .join(format!("{}-associations.json", job.id)),
            br#"{"likes":10,"reposts":2,"quotes":3,"replies":4}"#,
        )
        .unwrap();
        let claimed = queue.claim(120).unwrap().unwrap();
        queue
            .renew_lease(&claimed.id, claimed.lease_token.as_deref().unwrap(), 120)
            .unwrap();
        assert_eq!(queue.get(&job.id).unwrap().records_written, 19);
    }

    #[test]
    fn rejects_non_public_network_ranges() {
        for address in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "224.0.0.1",
            "240.0.0.1",
            "0.1.2.3",
            "::1",
            "fc00::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public_ip(address.parse().unwrap()), "accepted {address}");
        }
        assert!(public_ip("1.1.1.1".parse().unwrap()));
        assert!(public_ip("2606:4700:4700::1111".parse().unwrap()));
    }
}
