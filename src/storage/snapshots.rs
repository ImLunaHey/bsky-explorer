//! Small persisted responses: expensive archive scans never run on a page request.
use super::{Storage, StoredRecord, overview::Window};
use anyhow::Result;
use clickhouse::Row;
use serde::Deserialize;

#[derive(Clone, Row, Deserialize, serde::Serialize)]
pub struct RepositoryRecord {
    pub at_uri: String,
    pub actor_did: String,
    pub collection: String,
    pub operation: String,
    pub timestamp_ms: i64,
    pub record_json: String,
}

impl Storage {
    pub async fn repository_record(&self, uri: &str) -> Result<Option<RepositoryRecord>> {
        let owner = uri
            .strip_prefix("at://")
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("");
        Ok(self.client.query("SELECT at_uri, actor_did, collection, operation, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, record_json FROM events WHERE actor_did=? AND at_uri=? ORDER BY time_us DESC LIMIT 1 SETTINGS max_execution_time=5, max_threads=2")
            .bind(owner).bind(uri).fetch_optional::<RepositoryRecord>().await?)
    }

    pub async fn collection_records(&self, collection: &str) -> Result<Vec<RepositoryRecord>> {
        Ok(self.client.query("SELECT at_uri, actor_did, collection, operation, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, record_json FROM events WHERE collection=? AND timestamp >= now()-INTERVAL 1 DAY ORDER BY timestamp DESC, time_us DESC LIMIT 1 BY at_uri LIMIT 30 SETTINGS max_execution_time=5, max_threads=2")
            .bind(collection).fetch_all::<RepositoryRecord>().await?)
    }
    pub async fn snapshot(&self, key: &str) -> Result<Option<String>> {
        #[derive(Row, Deserialize)]
        struct Snapshot {
            payload: String,
        }
        Ok(self.client.query("SELECT payload FROM explorer_snapshots WHERE key=? ORDER BY generated_at DESC LIMIT 1 SETTINGS max_execution_time=2, max_threads=1")
            .bind(key).fetch_optional::<Snapshot>().await?.map(|row| row.payload))
    }

    pub async fn save_snapshot(&self, key: &str, payload: &str) -> Result<()> {
        self.client.query("INSERT INTO explorer_snapshots (key, generated_at, payload) VALUES (?, now64(3), ?)")
            .bind(key).bind(payload).execute().await?;
        Ok(())
    }

    pub async fn account_posts(&self, did: &str) -> Result<Vec<StoredRecord>> {
        Ok(self.client.query("SELECT at_uri, actor_did, toUnixTimestamp64Milli(timestamp) AS timestamp_ms, kind, text, reply_parent, reply_root, quote_target, urls, domains, languages FROM events WHERE actor_did=? AND kind IN ('post','reply','quote') AND operation != 'delete' ORDER BY timestamp DESC LIMIT 1 BY at_uri LIMIT 50 SETTINGS max_execution_time=5, max_threads=2")
            .bind(did).fetch_all::<StoredRecord>().await?)
    }
}

/// One serial worker, bounded queries, last-known-good data retained on failure.
/// A restart does not invalidate the persisted dashboard.
pub async fn refresh(storage: Storage) {
    let windows = [(Window::Hour, 60), (Window::Day, 300), (Window::Week, 900)];
    loop {
        for (window, ttl) in windows {
            let key = format!("atproto-overview:{}", window.seconds());
            let now = chrono::Utc::now().timestamp();
            let fresh = storage
                .snapshot(&key)
                .await
                .ok()
                .flatten()
                .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
                .and_then(|value| value["end"].as_i64())
                .is_some_and(|end| now - end < ttl);
            if fresh {
                continue;
            }
            match storage.overview(window, now).await {
                Ok(value) => {
                    if let Err(error) = storage
                        .save_snapshot(&key, &serde_json::to_string(&value).unwrap())
                        .await
                    {
                        tracing::warn!(%error, %key, "snapshot persistence failed");
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, %key, "snapshot refresh failed; retaining previous result")
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

pub async fn refresh_coverage(storage: Storage) {
    loop {
        let now = chrono::Utc::now().timestamp();
        let fresh = storage
            .snapshot("coverage")
            .await
            .ok()
            .flatten()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(&value).ok())
            .and_then(|value| value["generated_at"].as_i64())
            .is_some_and(|at| now - at < 1800);
        if !fresh {
            match storage.coverage().await {
                Ok(data) => {
                    let payload =
                        serde_json::json!({"generated_at": now, "data": data}).to_string();
                    if let Err(error) = storage.save_snapshot("coverage", &payload).await {
                        tracing::warn!(%error, "coverage snapshot persistence failed");
                    }
                }
                Err(error) => tracing::warn!(%error, "coverage refresh failed"),
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1800)).await;
    }
}
