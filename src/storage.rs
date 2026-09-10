use crate::{
    identity::IdentityRecord,
    model::{Detection, NormalizedEvent},
};
use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Storage {
    client: Client,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct TableStats {
    pub table: String,
    pub rows: u64,
    pub active_parts: u64,
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub compression_ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClickHouseStats {
    pub tables: Vec<TableStats>,
    pub pending_mutations: u64,
    pub errors_since_restart: u64,
    pub active_queries: u64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct AccountStoredStats {
    pub event_count: u64,
    pub post_count: u64,
    pub like_count: u64,
    pub repost_count: u64,
    pub follow_count: u64,
    pub unique_targets: u64,
    pub first_seen_ms: i64,
    pub last_seen_ms: i64,
    pub domains: Vec<String>,
    pub urls: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredRecord {
    pub at_uri: String,
    pub actor_did: String,
    pub timestamp_ms: i64,
    pub kind: String,
    pub text: String,
    pub reply_parent: String,
    pub reply_root: String,
    pub quote_target: String,
    pub urls: Vec<String>,
    pub domains: Vec<String>,
    pub languages: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredInteraction {
    pub event_id: String,
    pub actor_did: String,
    pub kind: String,
    pub timestamp_ms: i64,
    pub at_uri: String,
    pub target: String,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredAccountEvent {
    pub event_id: String,
    pub time_us: i64,
    pub actor_did: String,
    pub kind: String,
    pub at_uri: String,
    pub target: String,
    pub text: String,
}
#[derive(Debug, Clone, Default, Deserialize, Row, Serialize)]
pub struct FollowWindowStats {
    pub seconds: u64,
    pub outbound_follows: u64,
    pub inbound_follows: u64,
    pub outbound_unfollows_resolved: u64,
    pub inbound_unfollows_resolved: u64,
    pub outbound_unfollows_unresolved: u64,
    pub outbound_net: i64,
    pub inbound_net: i64,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct StoredFollowEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor_did: String,
    pub target_did: String,
    pub kind: String,
    pub direction: String,
    pub target_resolved: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct AccountFollowAnalytics {
    pub windows: Vec<FollowWindowStats>,
    pub recent_events: Vec<StoredFollowEvent>,
    pub unresolved_note: &'static str,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct SearchEvent {
    pub event_id: String,
    pub timestamp_ms: i64,
    pub actor_did: String,
    pub kind: String,
    pub collection: String,
    pub operation: String,
    pub at_uri: String,
    pub record_json: String,
    pub target: String,
    pub text: String,
    pub urls: Vec<String>,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct SearchIdentity {
    pub did: String,
    pub handle_value: String,
}
#[derive(Debug, Clone, Deserialize, Row)]
struct StoredUri {
    at_uri: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEventType {
    Records,
    Events,
    Both,
}
#[derive(Debug, Clone, Deserialize, Row, Serialize)]
pub struct RecordInteractionCounts {
    pub likes: u64,
    pub reposts: u64,
    pub replies: u64,
    pub quotes: u64,
}
#[derive(Row, Serialize)]
struct EventRow {
    event_id: String,
    time_us: i64,
    timestamp: i64,
    actor_did: String,
    kind: String,
    collection: String,
    rkey: String,
    operation: String,
    at_uri: String,
    record_json: String,
    target: String,
    target_did: String,
    text: String,
    reply_parent: String,
    reply_root: String,
    quote_target: String,
    urls: Vec<String>,
    domains: Vec<String>,
    languages: Vec<String>,
    text_hash: u64,
    normalized_text_hash: u64,
    simhash: u64,
}
impl From<&NormalizedEvent> for EventRow {
    fn from(e: &NormalizedEvent) -> Self {
        Self {
            event_id: e.event_id.clone(),
            time_us: e.time_us,
            timestamp: e.timestamp.timestamp_millis(),
            actor_did: e.actor_did.clone(),
            kind: e.kind.as_str().into(),
            collection: e.collection.clone(),
            rkey: e.rkey.clone(),
            operation: e.operation.clone(),
            at_uri: e.at_uri.clone(),
            record_json: e.record_json.clone().unwrap_or_default(),
            target: e.target.clone().unwrap_or_default(),
            target_did: e.target_did.clone().unwrap_or_default(),
            text: e.text.clone().unwrap_or_default(),
            reply_parent: e.reply_parent.clone().unwrap_or_default(),
            reply_root: e.reply_root.clone().unwrap_or_default(),
            quote_target: e.quote_target.clone().unwrap_or_default(),
            urls: e.urls.clone(),
            domains: e.domains.clone(),
            languages: e.languages.clone(),
            text_hash: e.text_hash.unwrap_or_default(),
            normalized_text_hash: e.normalized_text_hash.unwrap_or_default(),
            simhash: e.simhash.unwrap_or_default(),
        }
    }
}
impl Storage {
    pub fn new(url: &str, database: &str, user: &str, password: &str) -> Self {
        Self {
            client: Client::default()
                .with_url(url)
                .with_database(database)
                .with_user(user)
                .with_password(password),
        }
    }
    pub async fn insert_events(&self, events: &[NormalizedEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut insert = self.client.insert("events")?;
        for e in events {
            insert.write(&EventRow::from(e)).await?;
        }
        insert.end().await.context("commit ClickHouse event batch")
    }
    pub async fn ping(&self) -> Result<()> {
        self.client
            .query("SELECT 1")
            .execute()
            .await
            .context("ClickHouse readiness query")
    }
    pub async fn insert_detection(&self, d: &Detection) -> Result<()> {
        let body = serde_json::to_string(d)?;
        self.client.query("INSERT INTO detections (cluster_id, first_seen, last_seen, account_count, event_count, coordination_score, evidence_json) VALUES (?, fromUnixTimestamp64Milli(?), fromUnixTimestamp64Milli(?), ?, ?, ?, ?)").bind(&d.cluster_id).bind(d.first_seen.timestamp_millis()).bind(d.last_seen.timestamp_millis()).bind(d.accounts.len() as u32).bind(d.event_count as u64).bind(d.scores.coordination_score).bind(body).execute().await?;
        Ok(())
    }
    /// Loads the newest persisted version of a coordination detection.
    ///
    /// `cluster_id` is the leading sorting-key column, so this remains a
    /// narrow lookup even as the append-only detections table grows. Avoiding
    /// `FINAL` also prevents ClickHouse from merging every matching part on
    /// the request path; ordering by the replacement version gives us the
    /// same latest-row semantics.
    pub async fn detection(&self, cluster_id: &str) -> Result<Option<Detection>> {
        #[derive(Row, Deserialize)]
        struct DetectionJson {
            evidence_json: String,
        }

        let row = self
            .client
            .query(
                "SELECT evidence_json FROM detections WHERE cluster_id = ? ORDER BY last_seen DESC LIMIT 1",
            )
            .bind(cluster_id)
            .fetch_optional::<DetectionJson>()
            .await?;
        row.map(|row| serde_json::from_str(&row.evidence_json))
            .transpose()
            .context("decode persisted detection evidence")
    }
    pub async fn insert_identity(&self, identity: &IdentityRecord) -> Result<()> {
        for table in ["identity_history", "identities"] {
            self.client
                .query(&format!("INSERT INTO {table} (did, handle, observed_at, source) VALUES (?, ?, fromUnixTimestamp64Milli(?), ?)"))
                .bind(&identity.did)
                .bind(identity.handle.as_deref().unwrap_or(""))
                .bind(identity.observed_at.timestamp_millis())
                .bind(&identity.source)
                .execute()
                .await
                .with_context(|| format!("insert identity into {table}"))?;
        }
        Ok(())
    }
    pub async fn identity(&self, did: &str) -> Result<Option<IdentityRecord>> {
        #[derive(Row, Deserialize)]
        struct IdentityResult {
            did: String,
            handle: String,
            observed_at_ms: i64,
            source: String,
        }
        let row = self.client
            .query("SELECT did, handle, toUnixTimestamp64Milli(observed_at) AS observed_at_ms, source FROM identities FINAL WHERE did = ? LIMIT 1")
            .bind(did)
            .fetch_optional::<IdentityResult>()
            .await?;
        Ok(row.and_then(|row| {
            Some(IdentityRecord {
                did: row.did,
                handle: (!row.handle.is_empty()).then_some(row.handle),
                observed_at: chrono::DateTime::from_timestamp_millis(row.observed_at_ms)?,
                source: row.source,
            })
        }))
    }
    pub async fn identity_history(&self, did: &str, limit: u64) -> Result<Vec<IdentityRecord>> {
        #[derive(Row, Deserialize)]
        struct IdentityResult {
            did: String,
            handle: String,
            observed_at_ms: i64,
            source: String,
        }
        let rows = self.client
            .query("SELECT did, handle, toUnixTimestamp64Milli(observed_at) AS observed_at_ms, source FROM identity_history WHERE did = ? ORDER BY observed_at DESC LIMIT ?")
            .bind(did).bind(limit)
            .fetch_all::<IdentityResult>().await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                Some(IdentityRecord {
                    did: row.did,
                    handle: (!row.handle.is_empty()).then_some(row.handle),
                    observed_at: chrono::DateTime::from_timestamp_millis(row.observed_at_ms)?,
                    source: row.source,
                })
            })
            .collect())
    }
    pub async fn account_stats(&self, did: &str) -> Result<Option<AccountStoredStats>> {
        let row = self.client.query(
            "SELECT count() AS event_count, countIf(kind IN ('post','reply','quote')) AS post_count, countIf(kind='like') AS like_count, countIf(kind='repost') AS repost_count, countIf(kind='follow') AS follow_count, uniqExactIf(target, target != '') AS unique_targets, toUnixTimestamp64Milli(min(timestamp)) AS first_seen_ms, toUnixTimestamp64Milli(max(timestamp)) AS last_seen_ms, arraySlice(groupUniqArrayArray(100)(domains), 1, 50) AS domains, arraySlice(groupUniqArrayArray(100)(urls), 1, 50) AS urls FROM events WHERE actor_did = ? HAVING event_count > 0"
        ).bind(did).fetch_optional::<AccountStoredStats>().await?;
        Ok(row)
    }
    pub async fn account_recent_events(
        &self,
        did: &str,
        limit: u64,
    ) -> Result<Vec<StoredAccountEvent>> {
        Ok(self.client.query(
            "SELECT event_id, time_us, actor_did, kind, at_uri, target, text FROM events WHERE actor_did = ? ORDER BY time_us DESC LIMIT ?"
        ).bind(did).bind(limit.min(200))
        .fetch_all::<StoredAccountEvent>().await?)
    }
    /// Returns bounded follow analytics from normalized events. Jetstream delete
    /// commits normally omit the deleted follow record, so target-less outbound
    /// unfollows are reported separately instead of being misattributed.
    pub async fn account_follow_analytics(&self, did: &str) -> Result<AccountFollowAnalytics> {
        let window_query = self.client.query(
            "WITH ? AS account_did
             SELECT toUInt64(window_seconds) AS seconds,
               countIf(kind='follow' AND actor_did=account_did AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS outbound_follows,
               countIf(kind='follow' AND target_did=account_did AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS inbound_follows,
               countIf(kind='unfollow' AND actor_did=account_did AND target_did!='' AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS outbound_unfollows_resolved,
               countIf(kind='unfollow' AND target_did=account_did AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS inbound_unfollows_resolved,
               countIf(kind='unfollow' AND actor_did=account_did AND target_did='' AND timestamp >= now64(3)-toIntervalSecond(window_seconds)) AS outbound_unfollows_unresolved,
               toInt64(outbound_follows)-toInt64(outbound_unfollows_resolved)-toInt64(outbound_unfollows_unresolved) AS outbound_net,
               toInt64(inbound_follows)-toInt64(inbound_unfollows_resolved) AS inbound_net
             FROM events ARRAY JOIN [60,300,3600,86400] AS window_seconds
             WHERE collection='app.bsky.graph.follow'
               AND timestamp >= now64(3)-INTERVAL 1 DAY
               AND (actor_did=account_did OR target_did=account_did)
             GROUP BY window_seconds ORDER BY window_seconds"
        ).bind(did).fetch_all::<FollowWindowStats>();
        let events_query = self
            .client
            .query(
                "WITH ? AS account_did
             SELECT event_id, toUnixTimestamp64Milli(timestamp) AS timestamp_ms,
               actor_did, target_did, kind,
               if(actor_did=account_did,'outbound','inbound') AS direction,
               toBool(target_did!='') AS target_resolved
             FROM events
             WHERE collection='app.bsky.graph.follow'
               AND timestamp >= now64(3)-INTERVAL 30 DAY
               AND (actor_did=account_did OR target_did=account_did)
             ORDER BY timestamp DESC LIMIT 100",
            )
            .bind(did)
            .fetch_all::<StoredFollowEvent>();
        let (mut windows, recent_events) = tokio::try_join!(window_query, events_query)?;
        for seconds in [60, 300, 3_600, 86_400] {
            if !windows.iter().any(|window| window.seconds == seconds) {
                windows.push(FollowWindowStats {
                    seconds,
                    ..FollowWindowStats::default()
                });
            }
        }
        windows.sort_by_key(|window| window.seconds);
        Ok(AccountFollowAnalytics {
            windows,
            recent_events,
            unresolved_note: "Jetstream delete events may omit the original follow target. Those outbound unfollows are counted as unknown-target removals; they reduce outbound net change but cannot be attributed to a specific account. Inbound net change can therefore overstate growth when an incoming unfollow target was unavailable.",
        })
    }
    /// Searches retained normalized events. Exact identifiers search the full
    /// retention period; free-text searches are deliberately bounded to the
    /// last 30 days so an interactive request cannot scan every partition.
    pub async fn search_events(
        &self,
        term: &str,
        event_type: SearchEventType,
        limit: u64,
    ) -> Result<Vec<SearchEvent>> {
        let is_did = term.starts_with("did:");
        let is_at_uri = term.starts_with("at://");
        let is_url = term.starts_with("http://") || term.starts_with("https://");
        let exact_identifier = is_did || is_at_uri || is_url;
        let type_predicate = match event_type {
            SearchEventType::Records => "kind IN ('post', 'reply', 'quote')",
            SearchEventType::Events => "kind NOT IN ('post', 'reply', 'quote')",
            SearchEventType::Both => "1",
        };
        let time_predicate = if exact_identifier {
            "1"
        } else {
            "timestamp >= now() - INTERVAL 30 DAY"
        };
        let (search_predicate, bind_count) = if is_did {
            ("(actor_did = ? OR target_did = ?)", 2)
        } else if is_at_uri {
            (
                "(at_uri = ? OR target = ? OR reply_parent = ? OR reply_root = ? OR quote_target = ?)",
                5,
            )
        } else if is_url {
            ("(target = ? OR has(urls, ?))", 2)
        } else if term.contains('.') && !term.chars().any(char::is_whitespace) {
            (
                "(has(domains, ?) OR has(urls, ?) OR positionCaseInsensitiveUTF8(text, ?) > 0)",
                3,
            )
        } else {
            (
                "(target = ? OR positionCaseInsensitiveUTF8(text, ?) > 0 OR positionCaseInsensitiveUTF8(record_json, ?) > 0)",
                3,
            )
        };
        let sql = format!(
            "SELECT event_id, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, actor_did, kind, collection, operation, at_uri, record_json, target, text, urls, domains FROM events WHERE {type_predicate} AND {time_predicate} AND {search_predicate} ORDER BY timestamp DESC LIMIT ?"
        );
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "2")
            .with_option("max_threads", "2")
            .with_option("max_result_rows", (limit + 1).to_string());
        let mut query = client.query(&sql);
        for _ in 0..bind_count {
            query = query.bind(term);
        }
        Ok(query
            .bind(limit.min(10_001))
            .fetch_all::<SearchEvent>()
            .await?)
    }

    pub async fn search_identities(&self, term: &str, limit: u64) -> Result<Vec<SearchIdentity>> {
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "1")
            .with_option("max_threads", "2")
            .with_option("max_result_rows", (limit + 1).to_string());
        Ok(client
            .query(
                "SELECT did, argMax(handle, observed_at) AS handle_value FROM identities WHERE did = ? OR positionCaseInsensitiveUTF8(handle, ?) > 0 GROUP BY did ORDER BY (did = ?) DESC, handle_value, did LIMIT ?",
            )
            .bind(term)
            .bind(term)
            .bind(term)
            .bind(limit.min(10_001))
            .fetch_all::<SearchIdentity>()
            .await?)
    }
    pub async fn identities_for_dids(&self, dids: &[String]) -> Result<Vec<SearchIdentity>> {
        if dids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .client
            .query("SELECT did, argMax(handle, observed_at) AS handle_value FROM identities WHERE has(?, did) GROUP BY did")
            .bind(dids)
            .fetch_all::<SearchIdentity>()
            .await?)
    }
    pub async fn account_cluster_ids(&self, did: &str, limit: u64) -> Result<Vec<String>> {
        #[derive(Row, Deserialize)]
        struct ClusterResult {
            cluster_id: String,
        }
        // DIDs cannot contain JSON quote characters, so matching their encoded
        // account-array value avoids substring matches against evidence text.
        let needle = format!("%\"{}\"%", did);
        Ok(self.client
            .query("SELECT cluster_id FROM detections FINAL WHERE evidence_json LIKE ? ORDER BY last_seen DESC LIMIT ?")
            .bind(needle).bind(limit)
            .fetch_all::<ClusterResult>().await?
            .into_iter().map(|row| row.cluster_id).collect())
    }
    pub async fn record(&self, uri: &str) -> Result<Option<StoredRecord>> {
        Ok(self.client.query(
            "SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE at_uri = ? AND kind IN ('post','reply','quote') ORDER BY timestamp DESC LIMIT 1"
        ).bind(uri).fetch_optional::<StoredRecord>().await?)
    }
    pub async fn records(&self, uris: &[String]) -> Result<Vec<StoredRecord>> {
        if uris.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.client.query(
            "SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE has(?, at_uri) AND kind IN ('post','reply','quote') ORDER BY timestamp DESC LIMIT ?"
        ).bind(uris).bind(uris.len() as u64 * 2)
        .fetch_all::<StoredRecord>().await?)
    }
    pub async fn existing_uris(
        &self,
        uris: &[String],
    ) -> Result<std::collections::HashSet<String>> {
        if uris.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        Ok(self
            .client
            .query("SELECT DISTINCT at_uri FROM events WHERE has(?, at_uri)")
            .bind(uris)
            .fetch_all::<StoredUri>()
            .await?
            .into_iter()
            .map(|row| row.at_uri)
            .collect())
    }
    pub async fn record_interaction_counts(&self, uri: &str) -> Result<RecordInteractionCounts> {
        Ok(self.client.query(
            "SELECT countIf(kind='like' AND target=?) AS likes, countIf(kind='repost' AND target=?) AS reposts, countIf(kind='reply' AND reply_parent=?) AS replies, countIf(kind='quote' AND quote_target=?) AS quotes FROM events"
        ).bind(uri).bind(uri).bind(uri).bind(uri)
        .fetch_one::<RecordInteractionCounts>().await?)
    }
    pub async fn record_interactions(
        &self,
        uri: &str,
        limit: u64,
    ) -> Result<Vec<StoredInteraction>> {
        Ok(self.client.query(
            "SELECT event_id, actor_did, kind, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, at_uri, target FROM events WHERE (kind IN ('like','repost') AND target=?) OR (kind='reply' AND reply_parent=?) OR (kind='quote' AND quote_target=?) ORDER BY timestamp DESC LIMIT ?"
        ).bind(uri).bind(uri).bind(uri).bind(limit)
        .fetch_all::<StoredInteraction>().await?)
    }
    pub async fn conversation_context(&self, root: &str, limit: u64) -> Result<Vec<StoredRecord>> {
        Ok(self.client.query(
            "SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE at_uri=? OR reply_root=? ORDER BY timestamp ASC LIMIT ?"
        ).bind(root).bind(root).bind(limit)
        .fetch_all::<StoredRecord>().await?)
    }
    pub async fn bytes(&self) -> Result<u64> {
        self.table_bytes(None).await
    }
    pub async fn table_bytes(&self, table: Option<&str>) -> Result<u64> {
        let table = table.unwrap_or("");
        Ok(self.client.query("SELECT coalesce(sum(bytes_on_disk), 0) FROM system.parts WHERE active AND database=currentDatabase() AND (? = '' OR table = ?)").bind(table).bind(table).fetch_one::<u64>().await?)
    }
    pub async fn stats(&self) -> Result<ClickHouseStats> {
        let tables = self.client.query(
            "SELECT table, sum(rows) AS rows, count() AS active_parts, sum(bytes_on_disk) AS compressed_bytes, sum(data_uncompressed_bytes) AS uncompressed_bytes, if(compressed_bytes=0, 0., uncompressed_bytes/compressed_bytes) AS compression_ratio FROM system.parts WHERE active AND database=currentDatabase() GROUP BY table ORDER BY compressed_bytes DESC"
        ).fetch_all::<TableStats>().await?;
        let pending_mutations = self.client.query(
            "SELECT count() FROM system.mutations WHERE database=currentDatabase() AND NOT is_done"
        ).fetch_one::<u64>().await.unwrap_or(0);
        let errors_since_restart = self
            .client
            .query("SELECT coalesce(sum(value), 0) FROM system.errors")
            .fetch_one::<u64>()
            .await
            .unwrap_or(0);
        let active_queries = self
            .client
            .query("SELECT count() FROM system.processes")
            .fetch_one::<u64>()
            .await
            .unwrap_or(0);
        Ok(ClickHouseStats {
            tables,
            pending_mutations,
            errors_since_restart,
            active_queries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventKind;
    #[test]
    fn serializes_without_options() {
        let e = NormalizedEvent {
            event_id: "e".into(),
            time_us: 0,
            timestamp: chrono::Utc::now(),
            actor_did: "d".into(),
            kind: EventKind::Like,
            collection: "c".into(),
            rkey: "r".into(),
            operation: "create".into(),
            at_uri: "u".into(),
            record_json: None,
            target: None,
            target_did: None,
            text: None,
            reply_parent: None,
            reply_root: None,
            quote_target: None,
            urls: vec![],
            domains: vec![],
            languages: vec![],
            text_hash: None,
            normalized_text_hash: None,
            simhash: None,
        };
        let r = EventRow::from(&e);
        assert_eq!(r.target, "");
        assert_eq!(r.kind, "like")
    }

    #[test]
    fn schema_is_append_optimized_bounded_and_blob_free() {
        let schema = include_str!("../migrations/001_init.sql");
        assert!(schema.contains("ENGINE = MergeTree"));
        assert!(schema.contains("PARTITION BY toYYYYMM(timestamp)"));
        assert!(schema.contains("TTL timestamp + INTERVAL 365 DAY"));
        assert!(schema.contains("CODEC(ZSTD(3))"));

        let lowercase = schema.to_ascii_lowercase();
        for forbidden in [" image ", " avatar ", " banner ", " video ", " blob "] {
            assert!(
                !lowercase.contains(forbidden),
                "schema must not retain media column {forbidden:?}"
            );
        }
    }

    #[test]
    fn schema_matches_serialized_event_columns_in_order() {
        let schema = include_str!("../migrations/001_init.sql");
        let expected = [
            "event_id",
            "time_us",
            "timestamp",
            "actor_did",
            "kind",
            "collection",
            "rkey",
            "at_uri",
            "target",
            "target_did",
            "text",
            "reply_parent",
            "reply_root",
            "quote_target",
            "urls",
            "domains",
            "languages",
            "text_hash",
            "normalized_text_hash",
            "simhash",
        ];
        let event_table = schema
            .split("CREATE TABLE IF NOT EXISTS observatory.events")
            .nth(1)
            .unwrap()
            .split("ENGINE")
            .next()
            .unwrap();
        let mut previous = 0;
        for column in expected {
            let position = event_table[previous..]
                .find(column)
                .unwrap_or_else(|| panic!("missing or out-of-order ClickHouse column {column}"));
            previous += position + column.len();
        }
    }

    #[test]
    fn identity_schema_keeps_current_and_replayable_history() {
        let schema = include_str!("../migrations/002_identities.sql");
        assert!(schema.contains("identity_history"));
        assert!(schema.contains("ENGINE = ReplacingMergeTree(observed_at)"));
        assert!(schema.contains("TTL observed_at + INTERVAL 365 DAY"));
    }
}
