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
}

#[derive(Clone)]
pub struct IdentityQueue {
    db: Arc<Mutex<Connection>>,
    capacity: usize,
    negative_ttl: Duration,
}
impl IdentityQueue {
    pub fn open(root: PathBuf, capacity: usize, negative_ttl: Duration) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let mut db = Connection::open(root.join("queue.sqlite3"))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS identity_jobs(id TEXT PRIMARY KEY,did TEXT NOT NULL UNIQUE,status TEXT NOT NULL,created_at TEXT NOT NULL,updated_at TEXT NOT NULL,next_attempt_at TEXT,attempts INTEGER NOT NULL,max_attempts INTEGER NOT NULL,last_error TEXT,lease_token TEXT,lease_until TEXT); CREATE INDEX IF NOT EXISTS identity_jobs_claim ON identity_jobs(status,next_attempt_at,lease_until,created_at); CREATE INDEX IF NOT EXISTS identity_jobs_updated ON identity_jobs(updated_at DESC);")?;
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
        };
        upsert(&tx, &j)?;
        tx.commit()?;
        Ok(j)
    }
    pub fn list(&self, limit: usize, offset: usize) -> Vec<IdentityJob> {
        let db = self.db.lock();
        query_jobs(&db,"SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until FROM identity_jobs ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2",(limit.min(100) as i64, offset as i64)).unwrap_or_else(|e|{tracing::error!(%e,"list identity jobs");vec![]})
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
    ) -> Result<()> {
        j.status = status;
        j.updated_at = Utc::now();
        j.last_error = error;
        j.next_attempt_at = delay
            .and_then(|d| Duration::from_std(d).ok())
            .map(|d| Utc::now() + d);
        j.lease_token = None;
        j.lease_until = None;
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
    })
}
fn query_jobs(db: &Connection, sql: &str, paging: (i64, i64)) -> Result<Vec<IdentityJob>> {
    let mut q = db.prepare(sql)?;
    Ok(q.query_map(params![paging.0, paging.1], row_job)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
fn get_did(db: &Connection, did: &str) -> Result<Option<IdentityJob>> {
    Ok(db.query_row("SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until FROM identity_jobs WHERE did=?1",[did],row_job).optional()?)
}
fn get_id(db: &Connection, id: &str) -> Result<Option<IdentityJob>> {
    Ok(db.query_row("SELECT id,did,status,created_at,updated_at,next_attempt_at,attempts,max_attempts,last_error,lease_token,lease_until FROM identity_jobs WHERE id=?1",[id],row_job).optional()?)
}
fn upsert(db: &Connection, j: &IdentityJob) -> Result<()> {
    db.execute("INSERT INTO identity_jobs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) ON CONFLICT(id) DO UPDATE SET did=excluded.did,status=excluded.status,created_at=excluded.created_at,updated_at=excluded.updated_at,next_attempt_at=excluded.next_attempt_at,attempts=excluded.attempts,max_attempts=excluded.max_attempts,last_error=excluded.last_error,lease_token=excluded.lease_token,lease_until=excluded.lease_until",params![j.id,j.did,j.status.as_str(),stamp(j.created_at),stamp(j.updated_at),j.next_attempt_at.map(stamp),j.attempts,j.max_attempts,j.last_error,j.lease_token,j.lease_until.map(stamp)])?;
    Ok(())
}
fn import_json(root: &PathBuf, db: &mut Connection) -> Result<()> {
    let tx = db.transaction()?;
    let already: i64 = tx.query_row("SELECT count(*) FROM identity_jobs", [], |r| r.get(0))?;
    let mut imported_paths = Vec::new();
    if already == 0 {
        let mut imported = 0;
        for entry in fs::read_dir(root)? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) == Some("json")
                && let Ok(bytes) = fs::read(&path)
                && let Ok(j) = serde_json::from_slice::<IdentityJob>(&bytes)
            {
                upsert(&tx, &j)?;
                imported += 1;
                imported_paths.push(path);
            }
        }
        tracing::info!(
            imported,
            "imported legacy identity queue jobs into SQLite; JSON retained"
        );
    }
    tx.commit().context("commit identity queue JSON import")?;
    if !imported_paths.is_empty() {
        let backup = root.join("legacy-json-migration-backup");
        fs::create_dir_all(&backup)?;
        for path in imported_paths {
            let target = backup.join(
                path.file_name()
                    .context("legacy identity filename missing")?,
            );
            if !target.exists() {
                fs::rename(path, target)?
            }
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
    fn capacity_counts_only_active() {
        let d = tempdir().unwrap();
        let q = IdentityQueue::open(d.path().into(), 100, Duration::seconds(1)).unwrap();
        for n in 0..100 {
            q.enqueue(&format!("did:plc:test{n}")).unwrap();
        }
        assert!(q.enqueue("did:plc:overflow").is_err());
        let j = q.claim().unwrap().unwrap();
        q.finish(j, IdentityJobStatus::Resolved, None, None)
            .unwrap();
        q.enqueue("did:plc:new").unwrap();
        assert_eq!(q.summary().total, 101)
    }
}
