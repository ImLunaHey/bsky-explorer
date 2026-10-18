//! Bounded editorial-shaped feeds built in the enrichment worker, never on page load.
use super::{Storage, snapshots::RepositoryRecord};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pub const CONTENT_COLLECTIONS: [&str; 7] = [
    "site.standard.document",
    "com.whtwnd.blog.entry",
    "community.lexicon.calendar.event",
    "social.popfeed.feed.listItem",
    "is.currents.feed.save",
    "place.stream.livestream",
    "com.puzzmo.streak",
];

#[derive(Serialize)]
pub struct DiscoverySection {
    pub category: String,
    pub records: Vec<RepositoryRecord>,
    pub candidate_records: usize,
}

fn date(value: &Value, key: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value[key].as_str()?)
        .ok()
        .map(|d| d.timestamp())
}
fn web_url(raw: &str) -> Option<String> {
    let mut url = url::Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    url.set_fragment(None);
    Some(url.to_string())
}
fn eligible(record: &RepositoryRecord, now: i64) -> bool {
    if record.operation == "delete" {
        return false;
    }
    let Ok(value) = serde_json::from_str::<Value>(&record.record_json) else {
        return false;
    };
    if value["isDraft"].as_bool() == Some(true)
        || matches!(
            value["visibility"].as_str(),
            Some("private" | "unlisted" | "author")
        )
    {
        return false;
    }
    match record.collection.as_str() {
        "site.standard.document" | "com.whtwnd.blog.entry" => {
            !value["title"].as_str().unwrap_or("").trim().is_empty()
                && date(&value, "publishedAt")
                    .or_else(|| date(&value, "createdAt"))
                    .is_none_or(|at| at <= now && at >= now - 604800)
        }
        "community.lexicon.calendar.event" => {
            !value["name"].as_str().unwrap_or("").trim().is_empty()
                && date(&value, "startsAt").is_some_and(|at| at >= now && at < now + 90 * 86400)
                && !value["status"].as_str().unwrap_or("").contains("cancel")
                && value
                    .pointer("/preferences/showInDiscovery")
                    .and_then(Value::as_bool)
                    != Some(false)
        }
        "social.popfeed.feed.listItem" => {
            value["creativeWorkType"] == "movie"
                && !value["title"].as_str().unwrap_or("").is_empty()
        }
        "is.currents.feed.save" => value["originUrl"].as_str().and_then(web_url).is_some(),
        "place.stream.livestream" => {
            !value["title"].as_str().unwrap_or("").is_empty()
                && date(&value, "lastSeenAt").is_some_and(|at| at >= now - 86400 && at <= now)
        }
        "com.puzzmo.streak" => !value["gameDisplayName"].as_str().unwrap_or("").is_empty(),
        _ => false,
    }
}
fn canonical_key(record: &RepositoryRecord) -> String {
    let value: Value = serde_json::from_str(&record.record_json).unwrap_or_default();
    if record.collection == "site.standard.document" {
        if let (Some(site), Some(path)) = (value["site"].as_str(), value["path"].as_str()) {
            if let Ok(site) = url::Url::parse(site) {
                if let Ok(url) = site.join(path) {
                    if let Some(url) = web_url(url.as_str()) {
                        return url;
                    }
                }
            }
        }
    }
    if record.collection == "is.currents.feed.save" {
        if let Some(url) = value["originUrl"].as_str().and_then(web_url) {
            return url;
        }
    }
    // Do not merge film actions: watching and saving the same film differ.
    record.at_uri.clone()
}
fn compact(mut record: RepositoryRecord) -> RepositoryRecord {
    if let Ok(Value::Object(mut value)) = serde_json::from_str::<Value>(&record.record_json) {
        value.retain(|key, _| {
            [
                "title",
                "name",
                "textContent",
                "description",
                "content",
                "site",
                "path",
                "publishedAt",
                "createdAt",
                "startsAt",
                "endsAt",
                "locations",
                "uris",
                "url",
                "originUrl",
                "lastSeenAt",
                "tags",
                "mainCredit",
                "creativeWorkType",
                "listType",
                "posterUrl",
                "identifiers",
                "gameDisplayName",
                "current",
                "max",
                "total",
                "lastUpdated",
                "visibility",
            ]
            .contains(&key.as_str())
        });
        for item in value.values_mut() {
            if let Some(text) = item.as_str() {
                *item = Value::String(text.chars().take(2000).collect());
            }
            if let Value::Array(items) = item {
                items.truncate(6);
            }
        }
        record.record_json = Value::Object(value).to_string();
    }
    record
}
fn select(records: Vec<RepositoryRecord>, now: i64, events: bool) -> Vec<RepositoryRecord> {
    let mut seen = HashSet::new();
    let mut authors = HashMap::new();
    let mut selected: Vec<_> = records.into_iter().filter(|r| eligible(r, now)).collect();
    if events {
        selected.sort_by_key(|r| {
            serde_json::from_str::<Value>(&r.record_json)
                .ok()
                .and_then(|v| date(&v, "startsAt"))
                .unwrap_or(i64::MAX)
        });
    }
    selected
        .into_iter()
        .filter(|r| {
            if !seen.insert(canonical_key(r)) {
                return false;
            }
            let count = authors.entry(r.actor_did.clone()).or_insert(0);
            *count += 1;
            *count <= 3
        })
        .take(30)
        .map(compact)
        .collect()
}
impl Storage {
    pub async fn refresh_discovery(&self) -> Result<()> {
        let _permit = super::BACKGROUND_QUERY.acquire().await?;
        let now = chrono::Utc::now().timestamp();
        let client = self
            .client
            .clone()
            .with_option("max_execution_time", "30")
            .with_option("max_threads", "1")
            .with_option("max_memory_usage", "500000000")
            .with_option("max_block_size", "2048");
        let mut sections = Vec::new();
        for (category, collections) in [
            (
                "writing",
                vec![CONTENT_COLLECTIONS[0], CONTENT_COLLECTIONS[1]],
            ),
            ("events", vec![CONTENT_COLLECTIONS[2]]),
            ("films", vec![CONTENT_COLLECTIONS[3]]),
            ("bookmarks", vec![CONTENT_COLLECTIONS[4]]),
            ("streams", vec![CONTENT_COLLECTIONS[5]]),
            ("games", vec![CONTENT_COLLECTIONS[6]]),
        ] {
            // Pick identifiers first, then PREWHERE limits payload reads to those
            // identifiers. Sorting all article bodies can exceed memory budgets.
            let records = client.query("SELECT at_uri,actor_did,collection,operation,toUnixTimestamp64Milli(timestamp) AS timestamp_ms,record_json FROM events PREWHERE collection IN ? AND timestamp >= fromUnixTimestamp(?) AND timestamp < fromUnixTimestamp(?) AND at_uri IN (SELECT at_uri FROM events WHERE collection IN ? AND timestamp >= fromUnixTimestamp(?) AND timestamp < fromUnixTimestamp(?) ORDER BY timestamp DESC,time_us DESC LIMIT 1 BY at_uri LIMIT 800) ORDER BY timestamp DESC,time_us DESC LIMIT 1 BY at_uri LIMIT 800")
                .bind(&collections).bind(now-604800).bind(now).bind(&collections).bind(now-604800).bind(now).fetch_all::<RepositoryRecord>().await?;
            let candidate_records = records.len();
            sections.push(DiscoverySection {
                category: category.into(),
                records: select(records, now, category == "events"),
                candidate_records,
            });
        }
        self.save_snapshot("discovery", &serde_json::json!({"generated_at":now,"observed_since":now-604800,"sections":sections}).to_string()).await
    }
    pub async fn account_content(&self, did: &str) -> Result<Vec<RepositoryRecord>> {
        let collections: Vec<_> = CONTENT_COLLECTIONS
            .iter()
            .copied()
            .chain(super::music::COLLECTIONS.iter().copied())
            .collect();
        let records = self.client.query("SELECT at_uri,actor_did,collection,operation,toUnixTimestamp64Milli(timestamp) AS timestamp_ms,record_json FROM events WHERE actor_did=? AND collection IN ? ORDER BY timestamp DESC,time_us DESC LIMIT 1 BY at_uri LIMIT 90 SETTINGS max_execution_time=5,max_threads=2")
            .bind(did).bind(collections).fetch_all::<RepositoryRecord>().await?;
        Ok(records
            .into_iter()
            .filter(|r| {
                if r.operation == "delete" {
                    return false;
                }
                let v: Value = serde_json::from_str(&r.record_json).unwrap_or_default();
                v["isDraft"].as_bool() != Some(true)
                    && !matches!(
                        v["visibility"].as_str(),
                        Some("private" | "unlisted" | "author")
                    )
            })
            .take(60)
            .collect())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn record(collection: &str, value: Value) -> RepositoryRecord {
        RepositoryRecord {
            at_uri: "at://did:plc:a/test.record.item/1".into(),
            actor_did: "did:plc:a".into(),
            collection: collection.into(),
            operation: "create".into(),
            timestamp_ms: 0,
            record_json: value.to_string(),
        }
    }
    #[test]
    fn hides_drafts_deletes_cancelled_past_and_opted_out_events() {
        let now = 1792320000;
        assert!(!eligible(
            &record(
                CONTENT_COLLECTIONS[1],
                serde_json::json!({"title":"Draft","isDraft":true})
            ),
            now
        ));
        for extras in [
            serde_json::json!({"status":"calendar#cancelled"}),
            serde_json::json!({"preferences":{"showInDiscovery":false}}),
        ] {
            let mut value = serde_json::json!({"name":"Meetup","startsAt":"2026-10-20T12:00:00Z"});
            value
                .as_object_mut()
                .unwrap()
                .extend(extras.as_object().unwrap().clone());
            assert!(!eligible(&record(CONTENT_COLLECTIONS[2], value), now));
        }
        assert!(eligible(
            &record(
                CONTENT_COLLECTIONS[2],
                serde_json::json!({"name":"Meetup","startsAt":"2026-10-20T12:00:00Z"})
            ),
            now
        ));
        assert!(!eligible(
            &record(
                CONTENT_COLLECTIONS[2],
                serde_json::json!({"name":"Past","startsAt":"2020-10-20T12:00:00Z"})
            ),
            now
        ));
    }
    #[test]
    fn deduplicates_source_urls_without_merging_different_film_actions() {
        let a = record(
            CONTENT_COLLECTIONS[0],
            serde_json::json!({"title":"Article","site":"https://example.org","path":"/story"}),
        );
        let mut b = a.clone();
        b.at_uri = "another".into();
        assert_eq!(select(vec![a, b], 1792320000, false).len(), 1);
        let mut film = record(
            CONTENT_COLLECTIONS[3],
            serde_json::json!({"title":"Film","creativeWorkType":"movie","listType":"watchlist"}),
        );
        let first = canonical_key(&film);
        film.at_uri = "different-action".into();
        assert_ne!(first, canonical_key(&film));
    }
}
