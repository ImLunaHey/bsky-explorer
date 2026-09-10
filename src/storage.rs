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
    at_uri: String,
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
            at_uri: e.at_uri.clone(),
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
            at_uri: "u".into(),
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
