use crate::identity::{IdentityService, RemoteIdentityResult};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Arc};
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
impl IdentityJobStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Retrying => "retrying",
            Self::Resolved => "resolved",
            Self::Negative => "negative",
            Self::DeadLetter => "dead_letter",
        }
    }
    fn parse(v: &str) -> Result<Self> {
        Ok(match v {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "retrying" => Self::Retrying,
            "resolved" => Self::Resolved,
            "negative" => Self::Negative,
            "dead_letter" => Self::DeadLetter,
            _ => anyhow::bail!("invalid identity job status: {v}"),
        })
    }
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
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub handle: Option<String>,
}

#[derive(Clone)]
pub struct IdentityQueue {
    db: Arc<Mutex<Connection>>,
    capacity: usize,
    negative_ttl: Duration,
}
pub struct IdentityPageQuery<'a> {
    pub limit: usize,
    pub offset: usize,
    pub status: Option<&'a str>,
    pub search: Option<&'a str>,
    pub sort: &'a str,
    pub descending: bool,
    pub hours: u32,
    pub bucket_minutes: u32,
}
impl IdentityQueue {
    pub fn open(root: PathBuf, capacity: usize, negative_ttl: Duration) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let mut db = Connection::open(root.join("queue.sqlite3"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS identity_jobs(id TEXT PRIMARY KEY,did TEXT NOT NULL UNIQUE,status TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,next_attempt_at TEXT,attempts INTEGER NOT NULL,max_attempts INTEGER NOT NULL,last_error TEXT,lease_token TEXT,lease_until TEXT); CREATE INDEX IF NOT EXISTS identity_jobs_claim ON identity_jobs(status,next_attempt_at,lease_until,created_at); CREATE INDEX IF NOT EXISTS identity_jobs_updated ON identity_jobs(updated_at DESC);")?;
        ensure_column(&db, "identity_jobs", "started_at", "TEXT")?;
        ensure_column(&db, "identity_jobs", "completed_at", "TEXT")?;
        ensure_column(&db, "identity_jobs", "duration_ms", "INTEGER")?;
        ensure_column(&db, "identity_jobs", "handle", "TEXT")?;
        db.execute_batch("CREATE INDEX IF NOT EXISTS identity_jobs_status_updated ON identity_jobs(status,updated_at DESC); CREATE INDEX IF NOT EXISTS identity_jobs_did ON identity_jobs(did); CREATE INDEX IF NOT EXISTS identity_jobs_handle ON identity_jobs(handle);")?;
        import_json(&root, &mut db)?;
        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            capacity: capacity.max(100),
            negative_ttl,
        })
    }
    pub fn enqueue(&self, did: &str) -> Result<IdentityJob> {
        anyhow::ensure!(valid_did(did), "invalid DID");
        let now = Utc::now();
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        if let Some(j) = get_did(&tx, did)?
            && (j.status != IdentityJobStatus::Negative || now - j.updated_at < self.negative_ttl)
        {
            tx.commit()?;
            return Ok(j);
        }
        let active: i64 = tx.query_row(
            "SELECT count(*) FROM identity_jobs WHERE status IN ('queued','running','retrying')",
            [],
            |r| r.get(0),
        )?;
        anyhow::ensure!(
            active < self.capacity as i64,
            "identity queue capacity reached; retained history was not pruned"
        );
        let j = IdentityJob {
            id: format!("id-{:016x}", xxh3_64(did.as_bytes())),
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
            started_at: None,
            completed_at: None,
            duration_ms: None,
            handle: None,
        };
        upsert(&tx, &j)?;
        tx.commit()?;
        Ok(j)
    }
    pub fn list(&self, limit: usize, offset: usize) -> Vec<IdentityJob> {
        let db = self.db.lock();
        query_jobs(&db,"SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until,started_at,completed_at,duration_ms,handle FROM identity_jobs ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2",(limit.min(100) as i64, offset as i64)).unwrap_or_else(|e|{tracing::error!(%e,"list identity jobs");vec![]})
    }
    pub fn summary(&self) -> IdentityQueueSummary {
        let db = self.db.lock();
        let mut s = IdentityQueueSummary::default();
        let result = (|| -> Result<()> {
            let mut q = db.prepare("SELECT status,count(*) FROM identity_jobs GROUP BY status")?;
            let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, usize>(1)?)))?;
            for row in rows {
                let (k, n) = row?;
                s.total += n;
                match k.as_str() {
                    "queued" => s.queued = n,
                    "running" => s.running = n,
                    "retrying" => s.retrying = n,
                    "resolved" => s.resolved = n,
                    "negative" => s.negative = n,
                    "dead_letter" => s.dead_letter = n,
                    _ => {}
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            tracing::error!(%e,"summarize identity queue")
        };
        s
    }
    pub fn page(&self, input: IdentityPageQuery<'_>) -> Result<IdentityQueuePage> {
        let IdentityPageQuery {
            limit,
            offset,
            status,
            search,
            sort,
            descending,
            hours,
            bucket_minutes,
        } = input;
        let db = self.db.lock();
        let sort = match sort {
            "created_at" => "created_at",
            "duration_ms" => "duration_ms",
            "attempts" => "attempts",
            _ => "updated_at",
        };
        let order = if descending { "DESC" } else { "ASC" };
        let pattern = search.map(|v| format!("%{}%", v.replace('%', "\\%").replace('_', "\\_")));
        let where_sql = " WHERE (?1 IS NULL OR status=?1) AND (?2 IS NULL OR did LIKE ?2 ESCAPE '\\' OR handle LIKE ?2 ESCAPE '\\')";
        let total: i64 = db.query_row(
            &format!("SELECT count(*) FROM identity_jobs{where_sql}"),
            params![status, pattern],
            |r| r.get(0),
        )?;
        let sql = format!(
            "SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until,started_at,completed_at,duration_ms,handle FROM identity_jobs{where_sql} ORDER BY {sort} {order},id {order} LIMIT ?3 OFFSET ?4"
        );
        let mut q = db.prepare(&sql)?;
        let jobs = q
            .query_map(
                params![status, pattern, limit.min(100) as i64, offset as i64],
                row_job,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let summary = summary_db(&db)?;
        let analytics = analytics_db(
            &db,
            hours.clamp(1, 168),
            bucket_minutes.clamp(1, 60),
            &summary,
        )?;
        Ok(IdentityQueuePage {
            summary,
            jobs,
            total: total as usize,
            offset,
            limit: limit.min(100),
            has_more: offset + limit.min(100) < total as usize,
            analytics,
        })
    }
    pub fn get(&self, id: &str) -> Result<Option<IdentityJob>> {
        get_id(&self.db.lock(), id)
    }
    fn claim(&self) -> Result<Option<IdentityJob>> {
        let now = Utc::now();
        let mut db = self.db.lock();
        let tx = db.transaction()?;
        let id:Option<String>=tx.query_row("SELECT id FROM identity_jobs WHERE status IN ('queued','retrying') AND (next_attempt_at IS NULL OR next_attempt_at<=?1) AND (lease_until IS NULL OR lease_until<=?1) ORDER BY created_at LIMIT 1",[stamp(now)],|r|r.get(0)).optional()?;
        let Some(id) = id else {
            tx.commit()?;
            return Ok(None);
        };
        let token = format!("{:016x}", xxh3_64(format!("{id}{now}").as_bytes()));
        tx.execute("UPDATE identity_jobs SET status='running',attempts=attempts+1,updated_at=?2,lease_token=?3,lease_until=?4 WHERE id=?1",params![id,stamp(now),token,stamp(now+Duration::minutes(2))])?;
        tx.execute(
            "UPDATE identity_jobs SET started_at=COALESCE(started_at,?2) WHERE id=?1",
            params![id, stamp(now)],
        )?;
        let j = get_id(&tx, &id)?;
        tx.commit()?;
        Ok(j)
    }
    fn finish(
        &self,
        mut j: IdentityJob,
        status: IdentityJobStatus,
        error: Option<String>,
        delay: Option<std::time::Duration>,
        handle: Option<String>,
    ) -> Result<()> {
        j.status = status;
        j.updated_at = Utc::now();
        j.last_error = error;
        j.next_attempt_at = delay
            .and_then(|d| Duration::from_std(d).ok())
            .map(|d| Utc::now() + d);
        j.lease_token = None;
        j.lease_until = None;
        if matches!(
            j.status,
            IdentityJobStatus::Resolved
                | IdentityJobStatus::Negative
                | IdentityJobStatus::DeadLetter
        ) {
            j.completed_at = Some(j.updated_at);
            j.duration_ms = j
                .started_at
                .map(|v| (j.updated_at - v).num_milliseconds().max(0) as u64);
        }
        if handle.is_some() {
            j.handle = handle;
        }
        upsert(&self.db.lock(), &j)
    }
}
fn stamp(v: DateTime<Utc>) -> String {
    v.to_rfc3339()
}
fn parse_dt(v: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&v)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })
}
fn row_job(r: &rusqlite::Row<'_>) -> rusqlite::Result<IdentityJob> {
    let status: String = r.get(2)?;
    Ok(IdentityJob {
        id: r.get(0)?,
        did: r.get(1)?,
        status: IdentityJobStatus::parse(&status).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?,
        created_at: parse_dt(r.get(3)?)?,
        updated_at: parse_dt(r.get(4)?)?,
        next_attempt_at: r.get::<_, Option<String>>(5)?.map(parse_dt).transpose()?,
        attempts: r.get(6)?,
        max_attempts: r.get(7)?,
        last_error: r.get(8)?,
        lease_token: r.get(9)?,
        lease_until: r.get::<_, Option<String>>(10)?.map(parse_dt).transpose()?,
        started_at: r.get::<_, Option<String>>(11)?.map(parse_dt).transpose()?,
        completed_at: r.get::<_, Option<String>>(12)?.map(parse_dt).transpose()?,
        duration_ms: r.get(13)?,
        handle: r.get(14)?,
    })
}
fn query_jobs(db: &Connection, sql: &str, paging: (i64, i64)) -> Result<Vec<IdentityJob>> {
    let mut q = db.prepare(sql)?;
    Ok(q.query_map(params![paging.0, paging.1], row_job)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
fn get_did(db: &Connection, did: &str) -> Result<Option<IdentityJob>> {
    Ok(db.query_row("SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until,started_at,completed_at,duration_ms,handle FROM identity_jobs WHERE did=?1",[did],row_job).optional()?)
}
fn get_id(db: &Connection, id: &str) -> Result<Option<IdentityJob>> {
    Ok(db.query_row("SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until,started_at,completed_at,duration_ms,handle FROM identity_jobs WHERE id=?1",[id],row_job).optional()?)
}
fn upsert(db: &Connection, j: &IdentityJob) -> Result<()> {
    db.execute("INSERT INTO identity_jobs(id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until,started_at,completed_at,duration_ms,handle) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(id) DO UPDATE SET did=excluded.did,status=excluded.status,created_at=excluded.created_at,updated_at=excluded.updated_at,next_attempt_at=excluded.next_attempt_at,attempts=excluded.attempts,max_attempts=excluded.max_attempts,last_error=excluded.last_error,lease_token=excluded.lease_token,lease_until=excluded.lease_until,started_at=excluded.started_at,completed_at=excluded.completed_at,duration_ms=excluded.duration_ms,handle=excluded.handle",params![j.id,j.did,j.status.as_str(),stamp(j.created_at),stamp(j.updated_at),j.next_attempt_at.map(stamp),j.attempts,j.max_attempts,j.last_error,j.lease_token,j.lease_until.map(stamp),j.started_at.map(stamp),j.completed_at.map(stamp),j.duration_ms,j.handle])?;
    Ok(())
}
fn ensure_column(db: &Connection, table: &str, column: &str, kind: &str) -> Result<()> {
    let mut q = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let exists = q
        .query_map([], |r| r.get::<_, String>(1))?
        .any(|v| v.ok().as_deref() == Some(column));
    if !exists {
        db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"))?
    }
    Ok(())
}
fn import_json(root: &PathBuf, db: &mut Connection) -> Result<()> {
    let tx = db.transaction()?;
    let mut imported_paths = Vec::new();
    let mut imported = 0;
    let mut already_present = 0;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) == Some("json")
            && let Ok(bytes) = fs::read(&path)
            && let Ok(j) = serde_json::from_slice::<IdentityJob>(&bytes)
        {
            if get_did(&tx, &j.did)?.is_none() {
                upsert(&tx, &j)?;
                imported += 1;
            } else {
                already_present += 1;
            }
            anyhow::ensure!(
                get_did(&tx, &j.did)?.is_some(),
                "legacy identity job import verification failed"
            );
            imported_paths.push(path);
        }
    }
    tracing::info!(
        imported,
        already_present,
        "verified legacy identity queue jobs in SQLite"
    );
    tx.commit().context("commit identity queue JSON import")?;
    if !imported_paths.is_empty() {
        let backup = root.join("legacy-json-migration-backup");
        fs::create_dir_all(&backup)?;
        for path in imported_paths {
            let mut target = backup.join(
                path.file_name()
                    .context("legacy identity filename missing")?,
            );
            if target.exists() {
                let name = path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .context("legacy identity filename is not UTF-8")?;
                let mut suffix = 1_u32;
                loop {
                    let candidate = backup.join(format!("{name}.duplicate-{suffix}"));
                    if !candidate.exists() {
                        target = candidate;
                        break;
                    }
                    suffix += 1;
                }
            }
            fs::rename(path, target)?
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Default)]
pub struct IdentityQueueSummary {
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub retrying: usize,
    pub resolved: usize,
    pub negative: usize,
    pub dead_letter: usize,
}
#[derive(Debug, Serialize)]
pub struct IdentityQueuePage {
    pub summary: IdentityQueueSummary,
    pub jobs: Vec<IdentityJob>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
    pub analytics: IdentityQueueAnalytics,
}
#[derive(Debug, Serialize, Default)]
pub struct IdentityQueueAnalytics {
    pub avg_duration_ms: Option<f64>,
    pub p50_duration_ms: Option<u64>,
    pub p95_duration_ms: Option<u64>,
    pub p99_duration_ms: Option<u64>,
    pub success_rate: f64,
    pub negative_rate: f64,
    pub failure_rate: f64,
    pub throughput_1m: usize,
    pub throughput_5m: usize,
    pub throughput_1h: usize,
    pub throughput_24h: usize,
    pub backlog_eta_seconds: Option<u64>,
    pub oldest_queued_age_seconds: Option<u64>,
    pub avg_attempts: f64,
    pub max_attempts_observed: u32,
    pub series: Vec<IdentityTimeBucket>,
}
#[derive(Debug, Serialize)]
pub struct IdentityTimeBucket {
    pub bucket: DateTime<Utc>,
    pub resolved: usize,
    pub negative: usize,
    pub failures: usize,
    pub completed: usize,
    pub avg_duration_ms: Option<f64>,
}
fn summary_db(db: &Connection) -> Result<IdentityQueueSummary> {
    let mut s = IdentityQueueSummary::default();
    let mut q = db.prepare("SELECT status,count(*) FROM identity_jobs GROUP BY status")?;
    for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, usize>(1)?)))? {
        let (k, n) = row?;
        s.total += n;
        match k.as_str() {
            "queued" => s.queued = n,
            "running" => s.running = n,
            "retrying" => s.retrying = n,
            "resolved" => s.resolved = n,
            "negative" => s.negative = n,
            "dead_letter" => s.dead_letter = n,
            _ => {}
        }
    }
    Ok(s)
}
fn analytics_db(
    db: &Connection,
    hours: u32,
    bucket_minutes: u32,
    s: &IdentityQueueSummary,
) -> Result<IdentityQueueAnalytics> {
    let terminal = s.resolved + s.negative + s.dead_letter;
    let avg = db.query_row(
        "SELECT avg(duration_ms) FROM identity_jobs WHERE duration_ms IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let durations: i64 = db.query_row(
        "SELECT count(*) FROM identity_jobs WHERE duration_ms IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let percentile = |p: f64| -> Result<Option<u64>> {
        if durations == 0 {
            return Ok(None);
        }
        let offset = ((durations - 1) as f64 * p).round() as i64;
        Ok(db.query_row("SELECT duration_ms FROM identity_jobs WHERE duration_ms IS NOT NULL ORDER BY duration_ms LIMIT 1 OFFSET ?1",[offset],|r|r.get(0)).optional()?)
    };
    let count_since = |seconds: i64| -> Result<usize> {
        Ok(db.query_row(
            "SELECT count(*) FROM identity_jobs WHERE completed_at>=?1",
            [stamp(Utc::now() - Duration::seconds(seconds))],
            |r| r.get::<_, usize>(0),
        )?)
    };
    let t1 = count_since(60)?;
    let t5 = count_since(300)?;
    let t1h = count_since(3600)?;
    let t24 = count_since(86400)?;
    let oldest: Option<String> = db.query_row(
        "SELECT min(created_at) FROM identity_jobs WHERE status IN ('queued','retrying')",
        [],
        |r| r.get(0),
    )?;
    let oldest = oldest
        .and_then(|v| DateTime::parse_from_rfc3339(&v).ok())
        .map(|v| (Utc::now() - v.with_timezone(&Utc)).num_seconds().max(0) as u64);
    let backlog = s.queued + s.retrying;
    let eta = (t1h > 0 && backlog > 0).then(|| (backlog as f64 / (t1h as f64 / 3600.0)) as u64);
    let (avg_attempts, max_attempts): (f64, u32) = db.query_row(
        "SELECT coalesce(avg(attempts),0),coalesce(max(attempts),0) FROM identity_jobs",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let bucket_seconds = bucket_minutes as i64 * 60;
    let since = stamp(Utc::now() - Duration::hours(hours as i64));
    let mut q=db.prepare("SELECT (strftime('%s',completed_at)/?1)*?1 bucket_epoch,sum(status='resolved'),sum(status='negative'),sum(status='dead_letter'),count(*),avg(duration_ms) FROM identity_jobs WHERE completed_at>=?2 GROUP BY bucket_epoch ORDER BY bucket_epoch")?;
    let series = q
        .query_map(params![bucket_seconds, since], |r| {
            Ok(IdentityTimeBucket {
                bucket: DateTime::from_timestamp(r.get(0)?, 0).unwrap_or_default(),
                resolved: r.get(1)?,
                negative: r.get(2)?,
                failures: r.get(3)?,
                completed: r.get(4)?,
                avg_duration_ms: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(IdentityQueueAnalytics {
        avg_duration_ms: avg,
        p50_duration_ms: percentile(0.5)?,
        p95_duration_ms: percentile(0.95)?,
        p99_duration_ms: percentile(0.99)?,
        success_rate: if terminal > 0 {
            s.resolved as f64 / terminal as f64
        } else {
            0.0
        },
        negative_rate: if terminal > 0 {
            s.negative as f64 / terminal as f64
        } else {
            0.0
        },
        failure_rate: if terminal > 0 {
            s.dead_letter as f64 / terminal as f64
        } else {
            0.0
        },
        throughput_1m: t1,
        throughput_5m: t5,
        throughput_1h: t1h,
        throughput_24h: t24,
        backlog_eta_seconds: eta,
        oldest_queued_age_seconds: oldest,
        avg_attempts,
        max_attempts_observed: max_attempts,
        series,
    })
}
pub async fn run_worker(queue: IdentityQueue, service: IdentityService) {
    let mut idle_ticks = 0_u32;
    loop {
        match queue.claim() {
            Ok(Some(j)) => {
                let result = service.resolve_remote(&j.did).await;
                let (status, err, delay, handle) = match result {
                    Ok(RemoteIdentityResult::Resolved(identity)) => {
                        (IdentityJobStatus::Resolved, None, None, identity.handle)
                    }
                    Ok(RemoteIdentityResult::Negative) => {
                        (IdentityJobStatus::Negative, None, None, None)
                    }
                    Ok(RemoteIdentityResult::RetryAfter(d, e)) => {
                        (IdentityJobStatus::Retrying, Some(e), Some(d), None)
                    }
                    Err(e) => {
                        let base = (2u64.pow(j.attempts.min(8))).min(300);
                        let jitter = xxh3_64(format!("{}:{}", j.id, j.attempts).as_bytes()) % 1000;
                        let d = std::time::Duration::from_millis(base * 1000 + jitter);
                        if j.attempts >= j.max_attempts {
                            (
                                IdentityJobStatus::DeadLetter,
                                Some(e.to_string()),
                                None,
                                None,
                            )
                        } else {
                            (
                                IdentityJobStatus::Retrying,
                                Some(e.to_string()),
                                Some(d),
                                None,
                            )
                        }
                    }
                };
                if let Err(e) = queue.finish(j, status, err, delay, handle) {
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
                        Err(error) => tracing::debug!(%error,"identity discovery sweep failed"),
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
    use tempfile::tempdir;
    #[test]
    fn imports_json_and_preserves_it() {
        let d = tempdir().unwrap();
        let now = Utc::now();
        let j = IdentityJob {
            id: "id-old".into(),
            did: "did:plc:old".into(),
            status: IdentityJobStatus::Resolved,
            created_at: now,
            updated_at: now,
            next_attempt_at: None,
            attempts: 1,
            max_attempts: 8,
            last_error: None,
            lease_token: None,
            lease_until: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            handle: None,
        };
        fs::write(
            d.path().join("id-old.json"),
            serde_json::to_vec(&j).unwrap(),
        )
        .unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        assert_eq!(q.summary().resolved, 1);
        assert!(
            d.path()
                .join("legacy-json-migration-backup/id-old.json")
                .exists()
        );
        drop(q);
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        assert_eq!(q.summary().total, 1)
    }
    #[test]
    fn dedupes() {
        let d = tempdir().unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        assert_eq!(
            q.enqueue("did:plc:abc").unwrap().id,
            q.enqueue("did:plc:abc").unwrap().id
        );
        assert_eq!(q.summary().total, 1)
    }
    #[test]
    fn finishes_partial_prior_migration_without_overwriting_newer_state() {
        let d = tempdir().unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        let current = q.enqueue("did:plc:current").unwrap();
        drop(q);
        let mut stale = current.clone();
        stale.status = IdentityJobStatus::Resolved;
        let mut missing = current;
        missing.id = "id-missing".into();
        missing.did = "did:plc:missing".into();
        missing.status = IdentityJobStatus::Resolved;
        fs::write(
            d.path().join("id-current.json"),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        fs::write(
            d.path().join("id-missing.json"),
            serde_json::to_vec(&missing).unwrap(),
        )
        .unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        assert_eq!(
            q.summary().queued,
            1,
            "existing SQLite state must win over stale JSON"
        );
        assert_eq!(
            q.summary().resolved,
            1,
            "missing legacy job must be imported"
        );
        assert!(!d.path().join("id-current.json").exists());
        assert!(!d.path().join("id-missing.json").exists());
        assert!(
            d.path()
                .join("legacy-json-migration-backup/id-current.json")
                .exists()
        );
    }
    #[test]
    fn capacity_counts_only_active() {
        let d = tempdir().unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        for n in 0..100 {
            q.enqueue(&format!("did:plc:test{n}")).unwrap();
        }
        assert!(q.enqueue("did:plc:overflow").is_err());
        let j = q.claim().unwrap().unwrap();
        q.finish(j, IdentityJobStatus::Resolved, None, None, None)
            .unwrap();
        q.enqueue("did:plc:new").unwrap();
        assert_eq!(q.summary().total, 101)
    }
    #[test]
    fn paginates_filters_and_reports_resolution_analytics() {
        let d = tempdir().unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        q.enqueue("did:plc:alpha").unwrap();
        q.enqueue("did:plc:beta").unwrap();
        let job = q.claim().unwrap().unwrap();
        q.finish(
            job,
            IdentityJobStatus::Resolved,
            None,
            None,
            Some("alpha.test".into()),
        )
        .unwrap();
        let page = q
            .page(IdentityPageQuery {
                limit: 10,
                offset: 0,
                status: Some("resolved"),
                search: Some("alpha"),
                sort: "duration_ms",
                descending: true,
                hours: 24,
                bucket_minutes: 15,
            })
            .unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.jobs[0].handle.as_deref(), Some("alpha.test"));
        assert_eq!(page.analytics.throughput_24h, 1);
        assert_eq!(page.analytics.success_rate, 1.0);
        assert!(page.jobs[0].started_at.is_some());
        assert!(page.jobs[0].completed_at.is_some());
    }
}
