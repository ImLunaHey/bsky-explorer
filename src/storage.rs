use crate::model::{Detection, NormalizedEvent};
use anyhow::{Context, Result};
use clickhouse::{Client, Row};
use serde::Serialize;

#[derive(Clone)]
pub struct Storage {
    client: Client,
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
    pub async fn bytes(&self) -> Result<u64> {
        self.table_bytes(None).await
    }
    pub async fn table_bytes(&self, table: Option<&str>) -> Result<u64> {
        let table = table.unwrap_or("");
        Ok(self.client.query("SELECT coalesce(sum(bytes_on_disk), 0) FROM system.parts WHERE active AND database=currentDatabase() AND (? = '' OR table = ?)").bind(table).bind(table).fetch_one::<u64>().await?)
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
}
