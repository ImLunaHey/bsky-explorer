use super::*;

#[derive(Debug, Clone, Default, Deserialize, Serialize, Row)]
pub struct ProfileBucket {
    pub timestamp_ms: i64,
    pub gained: u64,
    pub lost: u64,
    pub net: i64,
    pub followed: u64,
    pub unfollowed: u64,
    pub unknown_unfollows: u64,
    pub posts: u64,
    pub replies: u64,
    pub likes: u64,
    pub reposts: u64,
    pub other: u64,
    pub likes_received: u64,
    pub reposts_received: u64,
    pub replies_received: u64,
}

#[derive(Serialize)]
pub struct ProfileHistory {
    pub start_ms: i64,
    pub end_ms: i64,
    pub bucket_ms: i64,
    pub buckets: Vec<ProfileBucket>,
    pub note: &'static str,
}

#[derive(Deserialize, Serialize, Row)]
pub struct ProfileHistoryEvent {
    pub timestamp_ms: i64,
    pub event_id: String,
    pub actor_did: String,
    pub target_did: String,
    pub kind: String,
}

pub fn history_window(days: u32, now: i64) -> (i64, i64, i64) {
    let days = match days {
        7 => 7,
        30 => 30,
        _ => 1,
    };
    let bucket = if days == 1 { 3_600_000 } else { 86_400_000 };
    let end = now / bucket * bucket + bucket;
    (end - days * 86_400_000, end, bucket)
}

impl Storage {
    pub async fn profile_history_events(
        &self,
        did: &str,
        start: i64,
        end: i64,
        category: &str,
    ) -> Result<Vec<ProfileHistoryEvent>> {
        let filter = match category {
            "audience" => "target_did=account_did AND kind IN ('follow','unfollow')",
            "following" => "actor_did=account_did AND kind IN ('follow','unfollow')",
            "response" => {
                "target_did=account_did AND actor_did!=account_did AND kind IN ('like','repost','reply')"
            }
            _ => "actor_did=account_did AND kind NOT IN ('follow','unfollow')",
        };
        self.client.clone().with_option("max_execution_time","3").with_option("max_threads","2")
            .with_option("max_memory_usage","250000000")
            .query(&format!("SELECT DISTINCT toUnixTimestamp64Milli(timestamp) AS timestamp_ms,event_id,actor_did,target_did,kind FROM account_history_events PREWHERE account_did=? AND timestamp>=fromUnixTimestamp64Milli(?) AND timestamp<fromUnixTimestamp64Milli(?) WHERE {filter} ORDER BY timestamp_ms DESC,event_id LIMIT 50"))
            .bind(did).bind(start).bind(end).fetch_all::<ProfileHistoryEvent>().await.map_err(Into::into)
    }
    pub async fn profile_history(&self, did: &str, days: u32) -> Result<ProfileHistory> {
        let now = Utc::now().timestamp_millis();
        let (start, end, bucket) = history_window(days, now);
        // Only narrow scalar columns, no record bodies. Explicit budgets prevent
        // popular profiles from occupying the server indefinitely. Exclude repo
        // imports: they are snapshots, not newly observed actions.
        let rows = self.client.clone()
            .with_option("max_execution_time", "5")
            .with_option("max_threads", "2")
            .with_option("max_memory_usage", "500000000")
            .query("WITH ? AS who, ? AS width
              SELECT intDiv(toUnixTimestamp64Milli(timestamp), width)*width AS timestamp_ms,
                uniqExactIf(event_id, target_did=who AND kind='follow') AS gained,
                uniqExactIf(event_id, target_did=who AND kind='unfollow') AS lost,
                toInt64(gained)-toInt64(lost) AS net,
                uniqExactIf(event_id, actor_did=who AND kind='follow') AS followed,
                uniqExactIf(event_id, actor_did=who AND kind='unfollow') AS unfollowed,
                uniqExactIf(event_id, actor_did=who AND kind='unfollow' AND target_did='') AS unknown_unfollows,
                uniqExactIf(event_id, actor_did=who AND kind IN ('post','quote')) AS posts,
                uniqExactIf(event_id, actor_did=who AND kind='reply') AS replies,
                uniqExactIf(event_id, actor_did=who AND kind='like') AS likes,
                uniqExactIf(event_id, actor_did=who AND kind='repost') AS reposts,
                uniqExactIf(event_id, actor_did=who AND kind NOT IN ('post','quote','reply','like','repost','follow','unfollow')) AS other,
                uniqExactIf(event_id, target_did=who AND actor_did!=who AND kind='like') AS likes_received,
                uniqExactIf(event_id, target_did=who AND actor_did!=who AND kind='repost') AS reposts_received,
                uniqExactIf(event_id, target_did=who AND actor_did!=who AND kind='reply') AS replies_received
              FROM account_history_events
              PREWHERE account_did=who AND timestamp >= fromUnixTimestamp64Milli(?) AND timestamp <= fromUnixTimestamp64Milli(?)
              WHERE operation IN ('create','delete')
              GROUP BY timestamp_ms ORDER BY timestamp_ms")
            .bind(did).bind(bucket).bind(start).bind(now)
            .fetch_all::<ProfileBucket>().await?;
        let mut buckets = Vec::new();
        for timestamp_ms in (start..end).step_by(bucket as usize) {
            buckets.push(
                rows.iter()
                    .find(|row| row.timestamp_ms == timestamp_ms)
                    .cloned()
                    .unwrap_or(ProfileBucket {
                        timestamp_ms,
                        ..Default::default()
                    }),
            );
        }
        Ok(ProfileHistory {
            start_ms: start,
            end_ms: now,
            bucket_ms: bucket,
            buckets,
            note: "Observed live events only; repository imports are excluded. Zero means no retained events, not verified inactivity. Lost followers include only unfollows with a known target, so net change may overstate growth. This is not a historical follower-total chart. The final bucket is still in progress.",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires isolated CLICKHOUSE_TEST_URL"]
    async fn history_counts_live_changes_without_imports_or_duplicates() {
        let url = std::env::var("CLICKHOUSE_TEST_URL").unwrap();
        let db = format!("profile_test_{}", std::process::id());
        let admin = Client::default().with_url(&url);
        for statement in include_str!("../../migrations/001_init.sql")
            .replace("observatory", &db)
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            admin.query(statement).execute().await.unwrap();
        }
        let storage = Storage::new(&url, &db, "default", "");
        for statement in include_str!("../../migrations/010_profile_history.sql")
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            storage.client.query(statement).execute().await.unwrap();
        }
        for (id, actor, target, kind, op) in [
            ("1", "other", "person", "follow", "create"),
            ("1", "other", "person", "follow", "create"),
            ("2", "other", "person", "unfollow", "delete"),
            ("3", "person", "", "unfollow", "delete"),
            ("4", "person", "other", "follow", "create"),
            ("5", "person", "", "post", "create"),
            ("6", "other", "person", "like", "create"),
            ("7", "person", "person", "like", "create"),
            ("8", "other", "person", "follow", "pds_backfill"),
            ("9", "person", "", "post", "update"),
            ("10", "other", "elsewhere", "follow", "create"),
        ] {
            storage.client.query("INSERT INTO events (event_id,actor_did,target_did,kind,operation,timestamp) VALUES (?,?,?,?,?,now64(3)-INTERVAL 1 SECOND)")
                .bind(id).bind(actor).bind(target).bind(kind).bind(op).execute().await.unwrap();
        }
        for (days, length) in [(1, 24), (7, 7), (30, 30)] {
            let result = storage.profile_history("person", days).await.unwrap();
            assert_eq!(result.buckets.len(), length);
            assert_eq!(result.buckets.iter().map(|r| r.gained).sum::<u64>(), 1);
            assert_eq!(result.buckets.iter().map(|r| r.lost).sum::<u64>(), 1);
            assert_eq!(
                result
                    .buckets
                    .iter()
                    .map(|r| r.unknown_unfollows)
                    .sum::<u64>(),
                1
            );
            assert_eq!(result.buckets.iter().map(|r| r.posts).sum::<u64>(), 1);
            assert_eq!(
                result.buckets.iter().map(|r| r.likes_received).sum::<u64>(),
                1
            );
        }
        assert!(
            storage
                .profile_history("absent", 1)
                .await
                .unwrap()
                .buckets
                .iter()
                .all(|r| r.gained == 0)
        );
        admin
            .query(&format!("DROP DATABASE {db}"))
            .execute()
            .await
            .unwrap();
    }
    #[test]
    fn windows_are_aligned_and_bounded() {
        for (days, expected) in [(1, 24), (7, 7), (30, 30), (999, 24)] {
            let (start, end, width) = history_window(days, 1_792_330_000_000);
            assert_eq!((end - start) / width, expected);
            assert_eq!(start % width, 0);
            assert!(start < 1_792_330_000_000 && end > 1_792_330_000_000);
        }
    }
}
