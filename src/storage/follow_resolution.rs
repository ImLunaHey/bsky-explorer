//! Derived, replayable unfollow attribution. Never mutates raw events or cursors.
use super::*;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Deserialize, Serialize, Row)]
pub struct FollowVersion {
    pub at_uri: String,
    pub time_us: i64,
    pub target_did: String,
    pub operation: String,
}
#[derive(Clone, Deserialize, Serialize, Row)]
pub struct Deletion {
    pub time_us: i64,
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor_did: String,
    pub at_uri: String,
}
#[derive(Default, Clone, Deserialize, Serialize)]
pub struct Cursor {
    pub time_us: i64,
    pub event_id: String,
}
#[derive(Default, Deserialize, Serialize)]
pub struct RepairState {
    pub live: Cursor,
    pub history: Cursor,
    pub seeded: HashSet<String>,
}
#[derive(Row, Serialize)]
struct Correction {
    account_did: String,
    timestamp: i64,
    event_id: String,
    actor_did: String,
    target_did: String,
    kind: String,
    operation: String,
}
#[derive(Row, Serialize)]
struct Resolution {
    event_id: String,
    time_us: i64,
    actor_did: String,
    target_did: String,
    at_uri: String,
    resolved_at: i64,
}

/// As-of lookup, including tombstones. Never guess across a previous deletion,
/// conflicting same-time subjects, or a creation observed after the deletion.
pub fn target_before(versions: &[FollowVersion], time_us: i64) -> Option<String> {
    let latest = versions
        .iter()
        .filter(|v| v.time_us < time_us)
        .map(|v| v.time_us)
        .max()?;
    let rows: Vec<_> = versions.iter().filter(|v| v.time_us == latest).collect();
    if rows
        .iter()
        .any(|v| v.operation == "delete" || !crate::identity_queue::valid_did(&v.target_did))
    {
        return None;
    }
    let target = &rows[0].target_did;
    rows.iter()
        .all(|v| v.target_did == *target)
        .then(|| target.clone())
}

impl Storage {
    pub async fn follow_seed_days(&self) -> Result<Vec<String>> {
        Ok(self.client.clone().with_option("max_execution_time","30").with_option("max_threads","2")
            .query("SELECT toString(toDate(timestamp)) AS day FROM events WHERE collection='app.bsky.graph.follow' GROUP BY day ORDER BY day DESC").fetch_all::<String>().await?)
    }
    pub async fn seed_follow_day(&self, day: &str) -> Result<()> {
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "30")
            .with_option("max_threads", "2")
            .with_option("max_memory_usage", "500000000");
        client.query("INSERT INTO follow_record_versions SELECT at_uri,time_us,toString(target_did),operation FROM events WHERE toDate(timestamp)=toDate(?) AND collection='app.bsky.graph.follow' AND at_uri!=''").bind(day).execute().await?;
        client.query("INSERT INTO follow_deletions SELECT time_us,event_id,timestamp,toString(actor_did),at_uri FROM events WHERE toDate(timestamp)=toDate(?) AND collection='app.bsky.graph.follow' AND operation='delete' AND target_did='' AND at_uri!=''").bind(day).execute().await?;
        Ok(())
    }
    pub async fn repair_follow_batch(&self, cursor: &Cursor) -> Result<(Cursor, usize)> {
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "10")
            .with_option("max_threads", "2")
            .with_option("max_memory_usage", "250000000");
        // Limit the ordered physical read before grouping: grouping the entire
        // backlog first defeats the batch budget and exhausts memory at scale.
        let rows=client.query("SELECT time_us,event_id,toUnixTimestamp64Milli(any(timestamp)) AS timestamp_ms,any(actor_did) AS actor_did,any(at_uri) AS at_uri FROM (SELECT * FROM follow_deletions WHERE (time_us,event_id)>(?,?) ORDER BY time_us,event_id LIMIT 500) GROUP BY time_us,event_id ORDER BY time_us,event_id").bind(cursor.time_us).bind(&cursor.event_id).fetch_all::<Deletion>().await?;
        let Some(last) = rows.last() else {
            return Ok((Cursor::default(), 0));
        };
        let next = Cursor {
            time_us: last.time_us,
            event_id: last.event_id.clone(),
        };
        let done: HashSet<String> = client
            .query(
                "SELECT event_id FROM unfollow_resolutions WHERE event_id IN ? GROUP BY event_id",
            )
            .bind(rows.iter().map(|r| r.event_id.clone()).collect::<Vec<_>>())
            .fetch_all::<String>()
            .await?
            .into_iter()
            .collect();
        let pending: Vec<_> = rows
            .iter()
            .filter(|r| !done.contains(&r.event_id))
            .collect();
        if pending.is_empty() {
            return Ok((next, 0));
        }
        let versions=client.query("SELECT at_uri,time_us,target_did,operation FROM follow_record_versions WHERE at_uri IN ? ORDER BY at_uri,time_us DESC LIMIT 100000").bind(pending.iter().map(|r|r.at_uri.clone()).collect::<Vec<_>>()).fetch_all::<FollowVersion>().await?;
        let mut by_uri: HashMap<String, Vec<FollowVersion>> = HashMap::new();
        for version in versions {
            by_uri
                .entry(version.at_uri.clone())
                .or_default()
                .push(version);
        }
        let mut corrections = Vec::new();
        let mut resolutions = Vec::new();
        for deletion in pending {
            let Some(target) = by_uri
                .get(&deletion.at_uri)
                .and_then(|v| target_before(v, deletion.time_us))
            else {
                continue;
            };
            // Publish first, then mark complete. A crash may replay a correction;
            // readers deduplicate event_id and prefer the attributed target.
            for account_did in [&deletion.actor_did, &target]
                .into_iter()
                .collect::<HashSet<_>>()
            {
                corrections.push(Correction {
                    account_did: account_did.clone(),
                    timestamp: deletion.timestamp_ms,
                    event_id: deletion.event_id.clone(),
                    actor_did: deletion.actor_did.clone(),
                    target_did: target.clone(),
                    kind: "unfollow".into(),
                    operation: "delete".into(),
                });
            }
            resolutions.push(Resolution {
                event_id: deletion.event_id.clone(),
                time_us: deletion.time_us,
                actor_did: deletion.actor_did.clone(),
                target_did: target,
                at_uri: deletion.at_uri.clone(),
                resolved_at: Utc::now().timestamp_millis(),
            });
        }
        if !corrections.is_empty() {
            let mut insert = client.insert("account_history_events")?;
            for row in &corrections {
                insert.write(row).await?;
            }
            insert.end().await?;
            let mut insert = client.insert("unfollow_resolutions")?;
            for row in &resolutions {
                insert.write(row).await?;
            }
            insert.end().await?;
        }
        Ok((next, resolutions.len()))
    }
    pub async fn follow_resolution_totals(&self) -> Result<(u64, u64)> {
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "10")
            .with_option("max_threads", "2");
        let total = client
            .query("SELECT uniqExact(event_id) FROM follow_deletions")
            .fetch_one::<u64>()
            .await?;
        let resolved = client
            .query("SELECT uniqExact(event_id) FROM unfollow_resolutions")
            .fetch_one::<u64>()
            .await?;
        Ok((total, resolved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires isolated CLICKHOUSE_TEST_URL"]
    async fn repairs_retained_and_live_deletes_idempotently() {
        let url = std::env::var("CLICKHOUSE_TEST_URL").unwrap();
        let db = format!("follow_test_{}", std::process::id());
        let admin = Client::default().with_url(&url);
        for sql in include_str!("../../migrations/001_init.sql")
            .replace("observatory", &db)
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            admin.query(sql).execute().await.unwrap();
        }
        let storage = Storage::new(&url, &db, "default", "");
        for sql in include_str!("../../migrations/010_profile_history.sql")
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            storage.client.query(sql).execute().await.unwrap();
        }
        let now = Utc::now().timestamp_micros() - 10_000_000;
        // A historical pair predates the materialized views; a second pair is live.
        for phase in 0..2 {
            if phase == 1 {
                for sql in include_str!("../../migrations/011_follow_resolution.sql")
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    storage.client.query(sql).execute().await.unwrap();
                }
            }
            for (suffix, op, target, kind, offset) in [
                ("c", "create", "did:plc:target", "follow", 0),
                ("d", "delete", "", "unfollow", 1),
            ] {
                storage.client.query("INSERT INTO events (event_id,time_us,timestamp,actor_did,target_did,kind,operation,collection,at_uri) VALUES (?,?,fromUnixTimestamp64Micro(?),?,?,?,?,?,?)")
                    .bind(format!("{phase}{suffix}")).bind(now+offset).bind(now+offset).bind("did:plc:actor").bind(target).bind(kind).bind(op).bind("app.bsky.graph.follow").bind(format!("at://did:plc:actor/app.bsky.graph.follow/{phase}")).execute().await.unwrap();
            }
        }
        for day in storage.follow_seed_days().await.unwrap() {
            storage.seed_follow_day(&day).await.unwrap();
            storage.seed_follow_day(&day).await.unwrap();
        }
        assert_eq!(
            storage
                .repair_follow_batch(&Cursor::default())
                .await
                .unwrap()
                .1,
            2
        );
        assert_eq!(
            storage
                .repair_follow_batch(&Cursor::default())
                .await
                .unwrap()
                .1,
            0
        );
        // Simulate a crash after publication but before completion was recorded.
        storage
            .client
            .query("TRUNCATE TABLE unfollow_resolutions")
            .execute()
            .await
            .unwrap();
        assert_eq!(
            storage
                .repair_follow_batch(&Cursor::default())
                .await
                .unwrap()
                .1,
            2
        );
        let h = storage.profile_history("did:plc:target", 1).await.unwrap();
        assert_eq!(h.buckets.iter().map(|r| r.lost).sum::<u64>(), 2);
        let actor = storage.profile_history("did:plc:actor", 1).await.unwrap();
        assert_eq!(
            actor
                .buckets
                .iter()
                .map(|r| r.unknown_unfollows)
                .sum::<u64>(),
            0
        );
        assert_eq!(actor.buckets.iter().map(|r| r.unfollowed).sum::<u64>(), 2);
        let stats = storage
            .account_follow_analytics("did:plc:target")
            .await
            .unwrap();
        assert_eq!(stats.windows.last().unwrap().inbound_unfollows_resolved, 2);
        let events = storage
            .profile_history_events(
                "did:plc:actor",
                now / 1000 - 1000,
                now / 1000 + 1000,
                "following",
            )
            .await
            .unwrap();
        assert_eq!(events.len(), 4);
        assert!(events.iter().all(|e| e.target_did == "did:plc:target"));
        let raw = storage
            .client
            .query("SELECT count() FROM events WHERE kind='unfollow' AND target_did=''")
            .fetch_one::<u64>()
            .await
            .unwrap();
        assert_eq!(raw, 2);
        admin
            .query(&format!("DROP DATABASE {db}"))
            .execute()
            .await
            .unwrap();
    }
    fn version(t: i64, target: &str, op: &str) -> FollowVersion {
        FollowVersion {
            at_uri: "at://did:plc:a/app.bsky.graph.follow/r".into(),
            time_us: t,
            target_did: target.into(),
            operation: op.into(),
        }
    }
    #[test]
    fn as_of_targets_respect_tombstones_conflicts_and_reuse() {
        let a = version(1, "did:plc:a", "create");
        assert_eq!(target_before(&[a.clone()], 2), Some("did:plc:a".into()));
        assert_eq!(target_before(&[a.clone()], 1), None);
        assert_eq!(
            target_before(&[a.clone(), version(2, "", "delete")], 3),
            None
        );
        assert_eq!(
            target_before(&[a.clone(), version(2, "did:plc:b", "update")], 3),
            Some("did:plc:b".into())
        );
        assert_eq!(
            target_before(&[a.clone(), version(1, "did:plc:b", "create")], 3),
            None
        );
        assert_eq!(target_before(&[a.clone(), a], 3), Some("did:plc:a".into()));
    }
}
