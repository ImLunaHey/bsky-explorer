use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{fs, net::IpAddr, path::PathBuf, sync::Arc};
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
    Paused,
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
    /// Repository (depth 0) may create only its own association job (depth 1).
    /// Participants discovered at depth 1 are identity-enriched, never crawled.
    #[serde(default)]
    pub crawl_depth: u8,
    #[serde(default = "default_max_crawl_depth")]
    pub max_depth: u8,
    #[serde(default = "default_crawl_scope")]
    pub scope: String,
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
    #[serde(default)]
    pub crawl_id: Option<String>,
    #[serde(default)]
    pub parent_job_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CrawlStatus {
    Queued,
    Running,
    Paused,
    Completed,
    Cancelled,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlLimits {
    pub max_accounts: u64,
    pub max_jobs: u64,
    pub max_records: u64,
    pub max_requests: u64,
}
impl Default for CrawlLimits {
    fn default() -> Self {
        Self {
            max_accounts: 1_000,
            max_jobs: 10_000,
            max_records: 10_000_000,
            max_requests: 1_000_000,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlRequest {
    pub id: String,
    pub root_query: String,
    pub root_did: Option<String>,
    pub status: CrawlStatus,
    pub max_depth: u8,
    pub relations: Vec<String>,
    pub limits: CrawlLimits,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackfillCheckpoint {
    pub kind: String,
    pub payload: serde_json::Value,
    pub updated_at: DateTime<Utc>,
}
fn default_max_crawl_depth() -> u8 {
    1
}
fn default_crawl_scope() -> String {
    "repository_own_associations".into()
}

#[derive(Clone)]
pub struct BackfillQueue {
    db_path: PathBuf,
    db: Arc<Mutex<Connection>>,
}

impl BackfillQueue {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let db_path = root.join("queue.sqlite3");
        let mut db = Connection::open(&db_path)?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS backfill_jobs(id TEXT PRIMARY KEY,status TEXT NOT NULL,job_type TEXT NOT NULL,updated_at TEXT NOT NULL,next_attempt_at TEXT,lease_until TEXT,depends_on TEXT,job_json TEXT NOT NULL); CREATE TABLE IF NOT EXISTS backfill_checkpoints(job_id TEXT PRIMARY KEY,kind TEXT NOT NULL,payload TEXT NOT NULL,updated_at TEXT NOT NULL); CREATE TABLE IF NOT EXISTS crawl_requests(id TEXT PRIMARY KEY,status TEXT NOT NULL,updated_at TEXT NOT NULL,payload TEXT NOT NULL);")?;
        ensure_backfill_column(&db, "next_attempt_at", "TEXT")?;
        ensure_backfill_column(&db, "lease_until", "TEXT")?;
        ensure_backfill_column(&db, "depends_on", "TEXT")?;
        ensure_backfill_column(&db, "crawl_id", "TEXT")?;
        ensure_backfill_column(&db, "parent_job_id", "TEXT")?;
        backfill_index_columns(&mut db)?;
        db.execute_batch("CREATE INDEX IF NOT EXISTS backfill_claim_v2 ON backfill_jobs(status,next_attempt_at,lease_until,updated_at); CREATE INDEX IF NOT EXISTS backfill_dependency_v2 ON backfill_jobs(depends_on); CREATE INDEX IF NOT EXISTS backfill_crawl_lineage ON backfill_jobs(crawl_id,parent_job_id); CREATE INDEX IF NOT EXISTS crawls_status_updated ON crawl_requests(status,updated_at DESC);")?;
        import_legacy_backfills(&root, &mut db)?;
        recover_expired_backfills(&mut db, Utc::now())?;
        Ok(Self {
            db_path,
            db: Arc::new(Mutex::new(db)),
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
        self.enqueue_with_lineage(
            query,
            did,
            kind,
            dependency,
            include_associations,
            None,
            None,
            0,
            1,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn enqueue_with_lineage(
        &self,
        query: &str,
        did: Option<&str>,
        kind: JobType,
        dependency: Option<String>,
        include_associations: bool,
        crawl_id: Option<String>,
        parent_job_id: Option<String>,
        crawl_depth: u8,
        max_depth: u8,
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
            xxh3_64(
                if let Some(crawl) = crawl_id.as_deref() {
                    format!("v2\0{crawl}\0{scope}\0{identity}")
                } else {
                    format!("v1\0{scope}\0{identity}")
                }
                .as_bytes()
            )
        );
        let db = self.db.lock();
        if let Some(job) = get_job(&db, &id)? {
            return Ok(job);
        }
        let active:i64=db.query_row("SELECT count(*) FROM backfill_jobs WHERE status IN ('queued','resolving','running','retrying')",[],|r|r.get(0))?;
        anyhow::ensure!(active < 100, "backfill queue is at its active-job limit");
        let now = Utc::now();
        let crawl_depth = crawl_depth.max(u8::from(matches!(kind, JobType::Associations)));
        let job = BackfillJob {
            id: id.clone(),
            query: query.into(),
            did: did.map(str::to_owned),
            handle: None,
            job_type: kind,
            status: JobStatus::Queued,
            depends_on_job_id: dependency.clone(),
            include_associations,
            crawl_depth,
            max_depth,
            scope: "repository_own_associations".into(),
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
            crawl_id,
            parent_job_id: parent_job_id.or(dependency),
        };
        persist_job(&db, &job)?;
        Ok(job)
    }
    pub fn create_crawl(
        &self,
        root: &str,
        did: Option<&str>,
        max_depth: u8,
        relations: Vec<String>,
        limits: CrawlLimits,
    ) -> Result<(CrawlRequest, BackfillJob)> {
        anyhow::ensure!(
            valid_backfill_query(root) && max_depth <= 3,
            "invalid crawl request"
        );
        let allowed = ["posts", "likes", "reposts", "quotes", "replies", "profiles"];
        anyhow::ensure!(
            !relations.is_empty() && relations.iter().all(|v| allowed.contains(&v.as_str())),
            "invalid crawl relations"
        );
        let now = Utc::now();
        let id = format!(
            "cr-{:016x}",
            xxh3_64(
                format!("{root}\0{}", now.timestamp_nanos_opt().unwrap_or_default()).as_bytes()
            )
        );
        let crawl = CrawlRequest {
            id: id.clone(),
            root_query: root.into(),
            root_did: did.map(str::to_owned),
            status: CrawlStatus::Queued,
            max_depth,
            relations: relations.clone(),
            limits,
            created_at: now,
            updated_at: now,
        };
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        persist_crawl(&tx, &crawl)?;
        tx.commit()?;
        drop(db);
        let include = relations
            .iter()
            .any(|v| matches!(v.as_str(), "likes" | "reposts" | "quotes" | "replies"));
        let job = self.enqueue_with_lineage(
            root,
            did,
            JobType::Repository,
            None,
            include,
            Some(id),
            None,
            0,
            max_depth,
        )?;
        Ok((crawl, job))
    }
    pub fn crawl(&self, id: &str) -> Result<Option<(CrawlRequest, Vec<BackfillJob>)>> {
        let db = self.db.lock();
        let Some(crawl) = get_crawl(&db, id)? else {
            return Ok(None);
        };
        let mut q=db.prepare("SELECT job_json FROM backfill_jobs WHERE crawl_id=?1 ORDER BY json_extract(job_json,'$.crawl_depth'),updated_at")?;
        let jobs = q
            .query_map([id], |r| r.get::<_, String>(0))?
            .map(|v| Ok(serde_json::from_str(&v?)?))
            .collect::<Result<_>>()?;
        Ok(Some((crawl, jobs)))
    }
    pub fn list_crawls(
        &self,
        limit: usize,
        offset: usize,
        status: Option<&str>,
    ) -> Result<(Vec<CrawlRequest>, usize)> {
        let db = self.db.lock();
        let total = db.query_row(
            "SELECT count(*) FROM crawl_requests WHERE (?1 IS NULL OR status=?1)",
            params![status],
            |r| r.get::<_, usize>(0),
        )?;
        let mut q=db.prepare("SELECT payload FROM crawl_requests WHERE (?1 IS NULL OR status=?1) ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3")?;
        let rows = q
            .query_map(params![status, limit.min(100), offset], |r| {
                r.get::<_, String>(0)
            })?
            .map(|v| Ok(serde_json::from_str(&v?)?))
            .collect::<Result<_>>()?;
        Ok((rows, total))
    }
    pub fn control(&self, id: &str, action: &str, scope: &str) -> Result<usize> {
        anyhow::ensure!(
            matches!(action, "pause" | "resume" | "cancel")
                && matches!(scope, "job" | "branch" | "crawl"),
            "invalid control"
        );
        let now = Utc::now();
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let root = get_job(&tx, id)?.context("job not found")?;
        if scope == "crawl" {
            let crawl_id = root.crawl_id.as_deref().context("job has no crawl")?;
            let status = match action {
                "pause" => CrawlStatus::Paused,
                "resume" => CrawlStatus::Running,
                _ => CrawlStatus::Cancelled,
            };
            let mut crawl = get_crawl(&tx, crawl_id)?.context("crawl not found")?;
            crawl.status = status;
            crawl.updated_at = now;
            persist_crawl(&tx, &crawl)?;
            if action == "cancel" {
                let ids: Vec<String> = {
                    let mut q=tx.prepare("SELECT id FROM backfill_jobs WHERE crawl_id=?1 AND status IN ('queued','paused','retrying')")?;
                    q.query_map([crawl_id], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?
                };
                for id in ids {
                    if let Some(mut j) = get_job(&tx, &id)? {
                        j.status = JobStatus::Cancelled;
                        j.updated_at = now;
                        persist_job(&tx, &j)?
                    }
                }
            }
            tx.commit()?;
            return Ok(1);
        }
        let ids = if scope == "branch" {
            let mut q=tx.prepare("WITH RECURSIVE branch(id) AS (SELECT ?1 UNION ALL SELECT j.id FROM backfill_jobs j JOIN branch b ON j.parent_job_id=b.id) SELECT id FROM branch")?;
            q.query_map([id], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?
        } else {
            vec![id.into()]
        };
        let mut changed = 0;
        for id in ids {
            if let Some(mut job) = get_job(&tx, &id)? {
                if job.status == JobStatus::Running && job.lease_until.is_some_and(|v| v > now) {
                    anyhow::bail!("leased running jobs cannot be controlled until lease expiry")
                };
                match action {
                    "pause" if matches!(job.status, JobStatus::Queued | JobStatus::Retrying) => {
                        job.status = JobStatus::Paused
                    }
                    "resume" if job.status == JobStatus::Paused => job.status = JobStatus::Queued,
                    "cancel"
                        if !matches!(job.status, JobStatus::Completed | JobStatus::Cancelled) =>
                    {
                        job.status = JobStatus::Cancelled
                    }
                    _ => continue,
                }
                job.updated_at = now;
                persist_job(&tx, &job)?;
                changed += 1
            }
        }
        tx.commit()?;
        Ok(changed)
    }
    pub fn control_crawl(&self, id: &str, action: &str) -> Result<usize> {
        anyhow::ensure!(
            matches!(action, "pause" | "resume" | "cancel"),
            "invalid crawl control"
        );
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let mut crawl = get_crawl(&tx, id)?.context("crawl not found")?;
        crawl.status = match action {
            "pause" => CrawlStatus::Paused,
            "resume" => CrawlStatus::Running,
            _ => CrawlStatus::Cancelled,
        };
        crawl.updated_at = Utc::now();
        persist_crawl(&tx, &crawl)?;
        if action == "cancel" {
            let ids: Vec<String> = {
                let mut q=tx.prepare("SELECT id FROM backfill_jobs WHERE crawl_id=?1 AND status IN ('queued','paused','retrying')")?;
                q.query_map([id], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            for job_id in &ids {
                if let Some(mut j) = get_job(&tx, job_id)? {
                    j.status = JobStatus::Cancelled;
                    j.updated_at = Utc::now();
                    persist_job(&tx, &j)?
                }
            }
        }
        let count = tx.query_row(
            "SELECT count(*) FROM backfill_jobs WHERE crawl_id=?1",
            [id],
            |r| r.get::<_, usize>(0),
        )?;
        tx.commit()?;
        Ok(count)
    }
    pub fn get(&self, id: &str) -> Option<BackfillJob> {
        get_job(&self.db.lock(), id).unwrap_or_else(|e| {
            tracing::error!(%e,"get backfill job");
            None
        })
    }
    pub fn list(&self, limit: usize) -> Vec<BackfillJob> {
        let db = self.db.lock();
        let result = (|| -> Result<Vec<_>> {
            let mut q =
                db.prepare("SELECT job_json FROM backfill_jobs ORDER BY updated_at DESC LIMIT ?1")?;
            Ok(
                q.query_map([limit.min(100) as i64], |r| r.get::<_, String>(0))?
                    .filter_map(|v| v.ok().and_then(|s| serde_json::from_str(&s).ok()))
                    .collect(),
            )
        })();
        result.unwrap_or_else(|e| {
            tracing::error!(%e,"list backfill jobs");
            vec![]
        })
    }
    pub fn list_page(
        &self,
        limit: usize,
        offset: usize,
        status: Option<&str>,
        job_type: Option<&str>,
    ) -> Result<(Vec<BackfillJob>, usize)> {
        let db = self.db.lock();
        let total:i64=db.query_row("SELECT count(*) FROM backfill_jobs WHERE (?1 IS NULL OR status=?1) AND (?2 IS NULL OR job_type=?2)",params![status,job_type],|r|r.get(0))?;
        let mut q=db.prepare("SELECT job_json FROM backfill_jobs WHERE (?1 IS NULL OR status=?1) AND (?2 IS NULL OR job_type=?2) ORDER BY updated_at DESC LIMIT ?3 OFFSET ?4")?;
        let jobs = q
            .query_map(
                params![status, job_type, limit.min(100) as i64, offset as i64],
                |r| r.get::<_, String>(0),
            )?
            .map(|v| Ok(serde_json::from_str(&v?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok((jobs, total as usize))
    }
    pub fn update(&self, job: BackfillJob) -> Result<()> {
        persist_job(&self.db.lock(), &job)
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
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        recover_expired_backfills_tx(&tx, now)?;
        let blocked: Vec<String> = {
            let mut q=tx.prepare("SELECT c.id FROM backfill_jobs c JOIN backfill_jobs p ON p.id=c.depends_on WHERE c.status='queued' AND p.status IN ('dead_letter','cancelled')")?;
            q.query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in blocked {
            if let Some(mut j) = get_job(&tx, &id)? {
                j.status = JobStatus::DeadLetter;
                j.last_error = Some("dependency did not complete".into());
                j.updated_at = now;
                persist_job(&tx, &j)?
            }
        }
        let ids: Vec<String> = {
            let mut q=tx.prepare("SELECT c.id FROM backfill_jobs c LEFT JOIN backfill_jobs p ON p.id=c.depends_on LEFT JOIN crawl_requests cr ON cr.id=c.crawl_id WHERE c.status IN ('queued','retrying','running') AND (c.next_attempt_at IS NULL OR c.next_attempt_at<=?1) AND (c.lease_until IS NULL OR c.lease_until<=?1) AND (c.depends_on IS NULL OR p.status='completed') AND (c.crawl_id IS NULL OR cr.status IN ('queued','running')) ORDER BY c.updated_at LIMIT 1")?;
            q.query_map([now.to_rfc3339()], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let selected = ids
            .into_iter()
            .filter_map(|id| get_job(&tx, &id).ok().flatten())
            .find(|j| {
                j.next_attempt_at.is_none_or(|t| t <= now) && j.lease_until.is_none_or(|t| t <= now)
            });
        if let Some(mut job) = selected {
            job.status = JobStatus::Running;
            job.updated_at = now;
            job.lease_token = Some(format!(
                "{:016x}",
                xxh3_64(format!("{}{}", job.id, now).as_bytes())
            ));
            job.lease_until = Some(now + Duration::seconds(lease_seconds));
            persist_job(&tx, &job)?;
            if let Some(crawl_id) = job.crawl_id.as_deref()
                && let Some(mut crawl) = get_crawl(&tx, crawl_id)?
                && crawl.status == CrawlStatus::Queued
            {
                crawl.status = CrawlStatus::Running;
                crawl.updated_at = now;
                persist_crawl(&tx, &crawl)?
            }
            tx.commit()?;
            Ok(Some(job))
        } else {
            tx.commit()?;
            Ok(None)
        }
    }
    fn renew_lease(&self, id: &str, token: &str, lease_seconds: i64) -> Result<()> {
        let checkpoint = self.checkpoint_progress(id);
        let mut job = self.get(id).context("leased job disappeared")?;
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
        self.update(job)
    }
    fn association_progress(&self, id: &str) -> Option<u64> {
        let value = self.checkpoint_value(id)?;
        Some(
            ["likes", "reposts", "quotes", "replies"]
                .iter()
                .filter_map(|key| value.get(key)?.as_u64())
                .sum(),
        )
    }
    fn checkpoint_progress(&self, id: &str) -> Option<(u64, Option<String>, Option<String>)> {
        if let Some(value) = self.checkpoint_value(id)
            && value.get("records_written").is_some()
        {
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
    fn checkpoint_value(&self, id: &str) -> Option<serde_json::Value> {
        self.db
            .lock()
            .query_row(
                "SELECT payload FROM backfill_checkpoints WHERE job_id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn checkpoint(&self, id: &str) -> Option<BackfillCheckpoint> {
        self.db
            .lock()
            .query_row(
                "SELECT kind,payload,updated_at FROM backfill_checkpoints WHERE job_id=?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|(kind, payload, updated_at)| {
                Some(BackfillCheckpoint {
                    kind,
                    payload: serde_json::from_str(&payload).ok()?,
                    updated_at: DateTime::parse_from_rfc3339(&updated_at)
                        .ok()?
                        .with_timezone(&Utc),
                })
            })
    }
}

fn status_name(s: &JobStatus) -> &'static str {
    match s {
        JobStatus::Queued => "queued",
        JobStatus::Paused => "paused",
        JobStatus::Resolving => "resolving",
        JobStatus::Running => "running",
        JobStatus::Retrying => "retrying",
        JobStatus::Completed => "completed",
        JobStatus::DeadLetter => "dead_letter",
        JobStatus::Cancelled => "cancelled",
    }
}
fn crawl_status_name(s: &CrawlStatus) -> &'static str {
    match s {
        CrawlStatus::Queued => "queued",
        CrawlStatus::Running => "running",
        CrawlStatus::Paused => "paused",
        CrawlStatus::Completed => "completed",
        CrawlStatus::Cancelled => "cancelled",
        CrawlStatus::Failed => "failed",
    }
}
fn persist_crawl(db: &Connection, c: &CrawlRequest) -> Result<()> {
    db.execute("INSERT INTO crawl_requests(id,status,updated_at,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET status=excluded.status,updated_at=excluded.updated_at,payload=excluded.payload",params![c.id,crawl_status_name(&c.status),c.updated_at.to_rfc3339(),serde_json::to_string(c)?])?;
    Ok(())
}
fn get_crawl(db: &Connection, id: &str) -> Result<Option<CrawlRequest>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM crawl_requests WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|v| serde_json::from_str(&v).map_err(Into::into))
        .transpose()
}
fn kind_name(k: &JobType) -> &'static str {
    match k {
        JobType::Repository => "repository",
        JobType::Associations => "associations",
    }
}
fn persist_job(db: &Connection, j: &BackfillJob) -> Result<()> {
    db.execute("INSERT INTO backfill_jobs(id,status,job_type,updated_at,next_attempt_at,lease_until,depends_on,job_json,crawl_id,parent_job_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(id) DO UPDATE SET status=excluded.status,job_type=excluded.job_type,updated_at=excluded.updated_at,next_attempt_at=excluded.next_attempt_at,lease_until=excluded.lease_until,depends_on=excluded.depends_on,job_json=excluded.job_json,crawl_id=excluded.crawl_id,parent_job_id=excluded.parent_job_id",params![j.id,status_name(&j.status),kind_name(&j.job_type),j.updated_at.to_rfc3339(),j.next_attempt_at.map(|v|v.to_rfc3339()),j.lease_until.map(|v|v.to_rfc3339()),j.depends_on_job_id,serde_json::to_string(j)?,j.crawl_id,j.parent_job_id])?;
    Ok(())
}
fn get_job(db: &Connection, id: &str) -> Result<Option<BackfillJob>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT job_json FROM backfill_jobs WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
        .transpose()
}
fn ensure_backfill_column(db: &Connection, column: &str, kind: &str) -> Result<()> {
    let mut q = db.prepare("PRAGMA table_info(backfill_jobs)")?;
    let exists = q
        .query_map([], |r| r.get::<_, String>(1))?
        .any(|v| v.ok().as_deref() == Some(column));
    if !exists {
        db.execute_batch(&format!(
            "ALTER TABLE backfill_jobs ADD COLUMN {column} {kind}"
        ))?
    }
    Ok(())
}
fn backfill_index_columns(db: &mut Connection) -> Result<usize> {
    let tx = db.transaction()?;
    let rows: Vec<(String, String)> = {
        let mut q = tx.prepare("SELECT id,job_json FROM backfill_jobs")?;
        q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?
    };
    let mut updated = 0;
    for (id, raw) in rows {
        if let Ok(job) = serde_json::from_str::<BackfillJob>(&raw) {
            tx.execute("UPDATE backfill_jobs SET status=?2,job_type=?3,updated_at=?4,next_attempt_at=?5,lease_until=?6,depends_on=?7,crawl_id=?8,parent_job_id=?9 WHERE id=?1",params![id,status_name(&job.status),kind_name(&job.job_type),job.updated_at.to_rfc3339(),job.next_attempt_at.map(|v|v.to_rfc3339()),job.lease_until.map(|v|v.to_rfc3339()),job.depends_on_job_id,job.crawl_id,job.parent_job_id])?;
            updated += 1
        }
    }
    tx.commit()?;
    Ok(updated)
}
fn recover_expired_backfills(db: &mut Connection, now: DateTime<Utc>) -> Result<usize> {
    let tx = db.transaction()?;
    let count = recover_expired_backfills_tx(&tx, now)?;
    tx.commit()?;
    Ok(count)
}
fn recover_expired_backfills_tx(db: &Connection, now: DateTime<Utc>) -> Result<usize> {
    let ids: Vec<String> = {
        let mut q=db.prepare("SELECT id FROM backfill_jobs WHERE status='running' AND (lease_until IS NULL OR lease_until<=?1)")?;
        q.query_map([now.to_rfc3339()], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?
    };
    let count = ids.len();
    for id in ids {
        if let Some(mut job) = get_job(db, &id)? {
            job.status = JobStatus::Queued;
            job.updated_at = now;
            job.next_attempt_at = None;
            job.lease_token = None;
            job.lease_until = None;
            persist_job(db, &job)?
        }
    }
    if count > 0 {
        tracing::warn!(count, "requeued backfill jobs with expired leases")
    };
    Ok(count)
}
fn import_legacy_backfills(root: &PathBuf, db: &mut Connection) -> Result<()> {
    let tx = db.transaction()?;
    let mut imported_paths = Vec::new();
    if tx.query_row("SELECT count(*) FROM backfill_jobs", [], |r| {
        r.get::<_, i64>(0)
    })? == 0
    {
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(&path) else { continue };
            if let Ok(mut j) = serde_json::from_slice::<BackfillJob>(&bytes) {
                j.crawl_depth = u8::from(matches!(j.job_type, JobType::Associations));
                j.max_depth = 1;
                j.scope = default_crawl_scope();
                persist_job(&tx, &j)?
            } else if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                let Some(name) = path.file_stem().and_then(|x| x.to_str()) else {
                    continue;
                };
                let (id, kind) = if let Some(id) = name.strip_suffix("-associations") {
                    (id, "associations")
                } else if let Some(id) = name.strip_suffix("-repository") {
                    (id, "repository")
                } else {
                    continue;
                };
                tx.execute(
                    "INSERT OR IGNORE INTO backfill_checkpoints VALUES(?1,?2,?3,?4)",
                    params![id, kind, payload.to_string(), Utc::now().to_rfc3339()],
                )?;
                imported_paths.push(path.clone());
            }
            if serde_json::from_slice::<BackfillJob>(&bytes).is_ok() {
                imported_paths.push(path);
            }
        }
    }
    tx.commit()?;
    if !imported_paths.is_empty() {
        let backup = root.join("legacy-json-migration-backup");
        fs::create_dir_all(&backup)?;
        for path in imported_paths {
            if path.exists() {
                let target =
                    backup.join(path.file_name().context("legacy queue filename missing")?);
                if !target.exists() {
                    fs::rename(path, target)?
                }
            }
        }
    }
    Ok(())
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
            let mut command = tokio::process::Command::new("/usr/local/bin/backfill_account");
            command.args([
                "--repo",
                &did,
                "--did",
                &did,
                "--pds",
                &pds,
                "--checkpoint",
                queue.db_path.to_str().context("invalid checkpoint path")?,
                "--checkpoint-job-id",
                &job.id,
            ]);
            command
        }
        JobType::Associations => {
            let mut command = tokio::process::Command::new("/usr/local/bin/backfill_associations");
            command.args([
                "--did",
                &did,
                "--checkpoint",
                queue.db_path.to_str().context("invalid checkpoint path")?,
                "--checkpoint-job-id",
                &job.id,
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
    if job.job_type == JobType::Repository
        && job.include_associations
        && job.crawl_depth < job.max_depth
    {
        queue.enqueue_with_lineage(
            &did,
            Some(&did),
            JobType::Associations,
            Some(job.id.clone()),
            false,
            job.crawl_id.clone(),
            Some(job.id),
            job.crawl_depth + 1,
            job.max_depth,
        )?;
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
        queue
            .db
            .lock()
            .execute(
                "INSERT INTO backfill_checkpoints VALUES(?1,'associations',?2,?3)",
                params![
                    job.id,
                    r#"{"likes":10,"reposts":2,"quotes":3,"replies":4}"#,
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
        let claimed = queue.claim(120).unwrap().unwrap();
        queue
            .renew_lease(&claimed.id, claimed.lease_token.as_deref().unwrap(), 120)
            .unwrap();
        assert_eq!(queue.get(&job.id).unwrap().records_written, 19);
        let checkpoint = queue.checkpoint(&job.id).expect("checkpoint detail");
        assert_eq!(checkpoint.kind, "associations");
        assert_eq!(checkpoint.payload["likes"], 10);
        assert!(checkpoint.updated_at <= Utc::now());
    }

    #[test]
    fn legacy_jobs_and_checkpoints_are_imported_and_archived() {
        let source = tempfile::tempdir().unwrap();
        let seed = BackfillQueue::open(source.path().into()).unwrap();
        let job = seed
            .enqueue(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                JobType::Associations,
                None,
                false,
            )
            .unwrap();
        drop(seed);
        let target = tempfile::tempdir().unwrap();
        fs::write(
            target.path().join(format!("{}.json", job.id)),
            serde_json::to_vec(&job).unwrap(),
        )
        .unwrap();
        fs::write(
            target.path().join(format!("{}-associations.json", job.id)),
            br#"{"likes":4,"reposts":3,"quotes":2,"replies":1}"#,
        )
        .unwrap();
        let queue = BackfillQueue::open(target.path().into()).unwrap();
        assert!(queue.get(&job.id).is_some());
        assert_eq!(queue.association_progress(&job.id), Some(10));
        let backup = target.path().join("legacy-json-migration-backup");
        assert!(backup.join(format!("{}.json", job.id)).exists());
        assert!(
            backup
                .join(format!("{}-associations.json", job.id))
                .exists()
        );
        assert!(!target.path().join(format!("{}.json", job.id)).exists());
    }
    #[test]
    fn upgrades_partial_sqlite_schema_and_recovers_expired_running_job() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("queue.sqlite3");
        let db = Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE backfill_jobs(id TEXT PRIMARY KEY,status TEXT NOT NULL,job_type TEXT NOT NULL,updated_at TEXT NOT NULL,depends_on TEXT,job_json TEXT NOT NULL); CREATE TABLE backfill_checkpoints(job_id TEXT PRIMARY KEY,kind TEXT NOT NULL,payload TEXT NOT NULL,updated_at TEXT NOT NULL);").unwrap();
        let now = Utc::now();
        let job = BackfillJob {
            id: "bf-aaaaaaaaaaaaaaaa".into(),
            query: "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa".into(),
            did: Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa".into()),
            handle: None,
            job_type: JobType::Repository,
            status: JobStatus::Running,
            depends_on_job_id: None,
            include_associations: false,
            crawl_depth: 0,
            max_depth: 1,
            scope: default_crawl_scope(),
            created_at: now - Duration::hours(1),
            updated_at: now - Duration::minutes(10),
            next_attempt_at: None,
            collection: Some("app.bsky.feed.post".into()),
            cursor: Some("cursor".into()),
            records_written: 42,
            retry_count: 2,
            max_retries: 8,
            last_error: None,
            lease_token: Some("old".into()),
            lease_until: Some(now - Duration::minutes(5)),
            crawl_id: None,
            parent_job_id: None,
        };
        db.execute(
            "INSERT INTO backfill_jobs VALUES(?1,'running','repository',?2,NULL,?3)",
            params![
                job.id,
                job.updated_at.to_rfc3339(),
                serde_json::to_string(&job).unwrap()
            ],
        )
        .unwrap();
        drop(db);
        let queue = BackfillQueue::open(directory.path().into()).unwrap();
        let recovered = queue.get(&job.id).unwrap();
        assert_eq!(recovered.status, JobStatus::Queued);
        assert_eq!(recovered.records_written, 42);
        assert_eq!(recovered.retry_count, 2);
        assert!(recovered.lease_until.is_none());
        assert!(queue.claim(120).unwrap().is_some());
    }
    #[test]
    fn crawl_lineage_and_controls_are_durable_and_lease_safe() {
        let d = tempfile::tempdir().unwrap();
        let q = BackfillQueue::open(d.path().into()).unwrap();
        let (crawl, root) = q
            .create_crawl(
                "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa",
                Some("did:plc:aaaaaaaaaaaaaaaaaaaaaaaa"),
                1,
                vec!["posts".into(), "likes".into()],
                CrawlLimits::default(),
            )
            .unwrap();
        assert_eq!(root.crawl_id.as_deref(), Some(crawl.id.as_str()));
        assert_eq!(root.crawl_depth, 0);
        assert_eq!(q.control(&root.id, "pause", "job").unwrap(), 1);
        assert_eq!(q.get(&root.id).unwrap().status, JobStatus::Paused);
        assert_eq!(q.control(&root.id, "resume", "job").unwrap(), 1);
        let running = q.claim(120).unwrap().unwrap();
        assert!(q.control(&running.id, "cancel", "job").is_err());
        assert_eq!(q.control_crawl(&crawl.id, "pause").unwrap(), 1);
        drop(q);
        let q = BackfillQueue::open(d.path().into()).unwrap();
        let (crawl2, jobs) = q.crawl(&crawl.id).unwrap().unwrap();
        assert_eq!(crawl2.status, CrawlStatus::Paused);
        assert_eq!(jobs.len(), 1);
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
