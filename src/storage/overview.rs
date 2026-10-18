//! Durable dashboard queries. Every panel uses the same half-open event-time window.
use super::Storage;
use anyhow::Result;
use clickhouse::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default)]
pub enum Window {
    #[serde(rename = "1h")]
    #[default]
    Hour,
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
}
impl Window {
    pub fn seconds(self) -> i64 {
        match self {
            Self::Hour => 3600,
            Self::Day => 86400,
            Self::Week => 604800,
        }
    }
    pub fn bucket(self) -> i64 {
        self.seconds() / 60
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Row)]
pub struct Activity {
    pub bucket: i64,
    pub events: u64,
    pub posts: u64,
    pub actors: u64,
    pub replies: u64,
}
#[derive(Debug, Clone, Deserialize, Serialize, Row)]
pub struct Totals {
    pub current: u8,
    pub events: u64,
    pub posts: u64,
    pub actors: u64,
    pub replies: u64,
    pub first_event: i64,
    pub last_event: i64,
}
#[derive(Debug, Clone, Deserialize, Serialize, Row)]
pub struct Breakdown {
    pub key: String,
    pub count: u64,
}
#[derive(Debug, Clone, Deserialize, Serialize, Row)]
pub struct Rising {
    pub uri: String,
    pub current: u64,
    pub previous: u64,
    pub growth: i64,
    pub participants: u64,
    pub text: String,
    pub actor: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    pub start: i64,
    pub end: i64,
    pub bucket_seconds: i64,
    pub totals: Vec<Totals>,
    pub activity: Vec<Activity>,
    pub languages: Vec<Breakdown>,
    pub domains: Vec<Breakdown>,
    pub collections: Vec<Breakdown>,
    pub posts: Vec<Rising>,
    pub conversations: Vec<Rising>,
}

impl Storage {
    pub async fn overview(&self, window: Window, end: i64) -> Result<Overview> {
        let _permit = super::BACKGROUND_QUERY.acquire().await?;
        let start = end - window.seconds();
        let previous = start - window.seconds();
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "90")
            .with_option("max_threads", "2")
            .with_option("max_memory_usage", "1000000000")
            .with_option("max_bytes_before_external_group_by", "128000000");
        // Explicit date predicates allow the existing event ordering to prune old data.
        let bounds = format!(
            "timestamp >= fromUnixTimestamp({previous}) AND timestamp < fromUnixTimestamp({end}) AND toDate(timestamp) >= toDate(fromUnixTimestamp({previous}))"
        );
        let current_bounds = format!("{bounds} AND timestamp >= fromUnixTimestamp({start})");
        let measures = "uniqCombined64(event_id) AS events, uniqCombined64If(event_id, collection='app.bsky.feed.post' AND kind IN ('post','reply','quote') AND operation='create') AS posts, uniqCombined64(actor_did) AS actors, uniqCombined64If(event_id, kind='reply' AND operation='create') AS replies";
        let collections = client.query(&format!("SELECT collection AS key, uniqCombined64(event_id) AS count FROM events WHERE {current_bounds} AND NOT startsWith(collection, 'app.bsky.') GROUP BY collection ORDER BY count DESC LIMIT 12")).fetch_all::<Breakdown>().await?;
        let totals = client.query(&format!("SELECT toUInt8(timestamp >= fromUnixTimestamp({start})) AS current, {measures}, toInt64(min(toUnixTimestamp(timestamp))) AS first_event, toInt64(max(toUnixTimestamp(timestamp))) AS last_event FROM events WHERE {bounds} GROUP BY current ORDER BY current")).fetch_all::<Totals>().await?;
        let bucket = window.bucket();
        let activity = client.query(&format!("SELECT toInt64({start} + intDiv(toInt64(toUnixTimestamp(timestamp)) - {start}, {bucket}) * {bucket}) AS bucket, {measures} FROM events WHERE {current_bounds} GROUP BY bucket ORDER BY bucket")).fetch_all::<Activity>().await?;
        let languages = client.query(&format!("SELECT language AS key, uniqCombined64(event_id) AS count FROM events ARRAY JOIN if(empty(languages), ['und'], arrayDistinct(languages)) AS language WHERE {current_bounds} AND kind IN ('post','reply','quote') AND operation='create' GROUP BY key ORDER BY count DESC LIMIT 12")).fetch_all::<Breakdown>().await?;
        let domains = client.query(&format!("SELECT domain AS key, uniqCombined64(event_id) AS count FROM events ARRAY JOIN arrayDistinct(domains) AS domain WHERE {current_bounds} AND kind IN ('post','reply','quote') AND operation='create' GROUP BY key ORDER BY count DESC LIMIT 8")).fetch_all::<Breakdown>().await?;
        let mut rankings = Vec::new();
        for conversations in [false, true] {
            let target = if conversations {
                "if(reply_root != '', reply_root, reply_parent)"
            } else {
                "multiIf(kind='reply', reply_parent, kind='quote', quote_target, target)"
            };
            let kinds = if conversations {
                "kind='reply'"
            } else {
                "kind IN ('like','repost','reply','quote')"
            };
            // Discover heavy hitters with bounded state before aggregating per URI.
            // Grouping every target on a busy week creates millions of tiny states.
            #[derive(Deserialize, Row)]
            struct Candidates {
                uris: Vec<String>,
            }
            let candidates = client.query(&format!("SELECT topK(1000)({target}) AS uris FROM events WHERE {bounds} AND {kinds} AND operation='create' AND startsWith({target}, 'at://') GROUP BY timestamp >= fromUnixTimestamp({start})")).fetch_all::<Candidates>().await?;
            let uris: Vec<_> = candidates
                .into_iter()
                .flat_map(|r| r.uris)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let minimum_people = if conversations { 2 } else { 1 };
            let query = format!(
                "SELECT {target} AS uri, uniqCombined64If(14)(event_id, timestamp >= fromUnixTimestamp({start})) AS current, uniqCombined64If(14)(event_id, timestamp < fromUnixTimestamp({start})) AS previous, toInt64(current)-toInt64(previous) AS growth, uniqCombined64If(14)(actor_did, timestamp >= fromUnixTimestamp({start})) AS participants, '' AS text, '' AS actor FROM events WHERE {bounds} AND {kinds} AND operation='create' AND {target} IN ? GROUP BY uri HAVING current > 0 AND growth > 0 AND participants >= {minimum_people} ORDER BY growth DESC, participants DESC, uri ASC LIMIT 8"
            );
            let mut rows = client
                .query(&query)
                .bind(uris)
                .fetch_all::<Rising>()
                .await?;
            // Bound preview lookup to the same two windows; older roots remain navigable.
            #[derive(Deserialize, Row)]
            struct Preview {
                uri: String,
                text: String,
                actor: String,
            }
            let uris: Vec<_> = rows.iter().map(|r| r.uri.clone()).collect();
            if !uris.is_empty() {
                let previews = client.query(&format!("SELECT at_uri AS uri, argMax(text, timestamp) AS text, argMax(actor_did, timestamp) AS actor FROM events WHERE {bounds} AND kind IN ('post','reply','quote') AND at_uri IN ? GROUP BY uri")).bind(uris).fetch_all::<Preview>().await?;
                for row in &mut rows {
                    row.actor = row
                        .uri
                        .strip_prefix("at://")
                        .unwrap_or("")
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .to_owned();
                    if let Some(p) = previews.iter().find(|p| p.uri == row.uri) {
                        row.text = p.text.clone();
                        row.actor = p.actor.clone();
                    }
                }
            }
            rankings.push(rows);
        }
        let conversations = rankings.pop().unwrap_or_default();
        let posts = rankings.pop().unwrap_or_default();
        Ok(Overview {
            start,
            end,
            bucket_seconds: bucket,
            totals,
            activity,
            languages,
            domains,
            collections,
            posts,
            conversations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires an isolated ClickHouse at CLICKHOUSE_TEST_URL"]
    async fn durable_overview_counts_boundaries_duplicates_and_rankings() {
        let url = std::env::var("CLICKHOUSE_TEST_URL").expect("isolated test server URL required");
        let db = format!("overview_test_{}", std::process::id());
        let admin = clickhouse::Client::default().with_url(&url);
        admin
            .query(&format!("CREATE DATABASE {db}"))
            .execute()
            .await
            .unwrap();
        let schema = include_str!("../../migrations/001_init.sql").replace("observatory", &db);
        for statement in schema.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            admin.query(statement).execute().await.unwrap();
        }
        let storage = Storage::new(&url, &db, "default", "");
        let end = 1_800_000_000_i64;
        let start = end - 3600;
        let insert = format!("INSERT INTO {db}.events (event_id, timestamp, time_us, actor_did, kind, collection, operation, at_uri, text, target, reply_root, reply_parent, languages, domains) VALUES
            ('old', fromUnixTimestamp({}), 0, 'did:plc:old', 'post', 'app.bsky.feed.post', 'create', 'at://did:plc:old/app.bsky.feed.post/p', 'old post', '', '', '', ['en'], []),
            ('post', fromUnixTimestamp({start}), 0, 'did:plc:a', 'post', 'app.bsky.feed.post', 'create', 'at://did:plc:a/app.bsky.feed.post/p', 'hello network', '', '', '', ['en','fr'], ['example.com']),
            ('post', fromUnixTimestamp({start}), 0, 'did:plc:a', 'post', 'app.bsky.feed.post', 'create', 'at://did:plc:a/app.bsky.feed.post/p', 'hello network', '', '', '', ['en','fr'], ['example.com']),
            ('like', fromUnixTimestamp({}), 0, 'did:plc:b', 'like', 'app.bsky.feed.like', 'create', '', '', 'at://did:plc:a/app.bsky.feed.post/p', '', '', [], []),
            ('reply', fromUnixTimestamp({}), 0, 'did:plc:c', 'reply', 'app.bsky.feed.post', 'create', 'at://did:plc:c/app.bsky.feed.post/r', 'a reply', '', 'at://did:plc:a/app.bsky.feed.post/p', 'at://did:plc:a/app.bsky.feed.post/p', [], []),
            ('reply2', fromUnixTimestamp({start}), 0, 'did:plc:f', 'reply', 'app.bsky.feed.post', 'create', 'at://did:plc:f/app.bsky.feed.post/r', 'another reply', '', 'at://did:plc:a/app.bsky.feed.post/p', 'at://did:plc:a/app.bsky.feed.post/p', [], []),
            ('excluded', fromUnixTimestamp({end}), 0, 'did:plc:d', 'post', 'app.bsky.feed.post', 'create', '', 'boundary', '', '', '', [], []),
            ('other-app', fromUnixTimestamp({start}), 0, 'did:plc:e', 'record', 'com.example.post', 'create', '', 'other app', '', '', '', [], [])", start - 1, start + 1, start + 2);
        admin.query(&insert).execute().await.unwrap();
        for statement in include_str!("../../migrations/009_explorer_performance.sql")
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            storage.client.query(statement).execute().await.unwrap();
        }
        let uri = "at://did:plc:a/app.bsky.feed.post/p".to_owned();
        assert_eq!(
            storage.record(&uri).await.unwrap().unwrap().text,
            "hello network"
        );
        assert_eq!(storage.account_posts("did:plc:a").await.unwrap().len(), 1);
        assert_eq!(
            storage
                .existing_uris(std::slice::from_ref(&uri))
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            storage
                .existing_event_ids(&["post".into()], &["did:plc:a".into()])
                .await
                .unwrap()
                .len(),
            1
        );
        let counts = storage.record_interaction_counts(&uri).await.unwrap();
        assert_eq!((counts.likes, counts.replies), (1, 2));
        // The client switches long SELECTs to readonly HTTP POST. Limits must
        // be HTTP options, not SQL SETTINGS (which readonly mode rejects).
        let mut batch: Vec<_> = (0..300)
            .map(|i| format!("at://did:plc:a/app.bsky.feed.post/not-retained-{i:08}"))
            .collect();
        batch.push(uri.clone());
        assert_eq!(storage.existing_uris(&batch).await.unwrap().len(), 1);
        assert!(!storage.records(&batch).await.unwrap().is_empty());
        let search = storage
            .search_events("hello", crate::storage::SearchEventType::Records, 10)
            .await
            .unwrap();
        assert!(!search.is_empty());
        assert!(search.iter().all(|row| row.record_json.is_empty()));
        let mut ids: Vec<_> = (0..300)
            .map(|i| format!("large-association-event-identifier-not-retained-{i:08}"))
            .collect();
        ids.push("post".into());
        assert_eq!(
            storage
                .existing_event_ids(&ids, &["did:plc:a".into()])
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(storage.snapshot("missing").await.unwrap().is_none());
        storage
            .save_snapshot("test", "{\"ready\":true}")
            .await
            .unwrap();
        assert_eq!(
            storage.snapshot("test").await.unwrap().unwrap(),
            "{\"ready\":true}"
        );
        storage.client.query("INSERT INTO detections (cluster_id, first_seen, last_seen, evidence_json) VALUES ('cluster-a', now64(3), now64(3), '{\"accounts\":[\"did:plc:a\"]}')").execute().await.unwrap();
        assert_eq!(
            storage.account_cluster_ids("did:plc:a", 50).await.unwrap(),
            vec!["cluster-a"]
        );
        assert!(
            storage
                .account_cluster_ids("did:plc:b", 50)
                .await
                .unwrap()
                .is_empty()
        );
        let result = storage.overview(Window::Hour, end).await.unwrap();
        let current = result.totals.iter().find(|r| r.current == 1).unwrap();
        assert_eq!(
            (
                current.events,
                current.posts,
                current.actors,
                current.replies
            ),
            (5, 3, 5, 2)
        );
        assert_eq!(
            result
                .totals
                .iter()
                .find(|r| r.current == 0)
                .unwrap()
                .events,
            1
        );
        assert_eq!(result.activity.iter().map(|r| r.events).sum::<u64>(), 5);
        assert_eq!(result.collections[0].key, "com.example.post");
        assert_eq!(result.posts[0].current, 3);
        assert_eq!(result.posts[0].text, "hello network");
        assert_eq!(result.conversations[0].current, 2);
        assert_eq!(result.domains[0].count, 1);
        assert_eq!(result.languages.len(), 3);
        assert_eq!(
            result
                .languages
                .iter()
                .find(|r| r.key == "und")
                .unwrap()
                .count,
            2
        );
        for window in [Window::Day, Window::Week] {
            assert_eq!(
                storage
                    .overview(window, end)
                    .await
                    .unwrap()
                    .totals
                    .iter()
                    .find(|r| r.current == 1)
                    .unwrap()
                    .events,
                6
            );
        }
        admin
            .query(&format!("DROP DATABASE {db}"))
            .execute()
            .await
            .unwrap();
    }
    #[test]
    fn windows_have_sixty_equal_buckets_and_reject_unknown_input() {
        for value in ["1h", "24h", "7d"] {
            let window: Window = serde_json::from_value(serde_json::json!(value)).unwrap();
            assert_eq!(window.seconds() / window.bucket(), 60);
        }
        assert!(serde_json::from_value::<Window>(serde_json::json!("100d")).is_err());
    }
}
