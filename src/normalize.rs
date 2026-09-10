use crate::model::{EventKind, NormalizedEvent};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use url::Url;
use xxhash_rust::xxh3::xxh3_64;

/// Generic records are useful for future lexicons and replay, but a malicious
/// repository must not be able to turn one event into an unbounded row.
pub const MAX_RECORD_JSON_BYTES: usize = 64 * 1024;
const MAX_JSON_STRING_BYTES: usize = 4096;
const MAX_JSON_CONTAINER_ITEMS: usize = 256;
const MAX_JSON_DEPTH: usize = 32;

fn is_blob_object(object: &serde_json::Map<String, Value>) -> bool {
    object.get("$type").and_then(Value::as_str) == Some("blob")
        || object
            .get("ref")
            .and_then(Value::as_object)
            .is_some_and(|reference| reference.contains_key("$link"))
            && (object.contains_key("mimeType") || object.contains_key("size"))
        || object.contains_key("cid") && object.contains_key("mimeType")
}

/// Clone public record metadata while dropping blob objects and obvious inline
/// binary payloads. `None` means the value itself was a blob.
fn sanitize_record(value: &Value, depth: usize) -> Option<Value> {
    if depth >= MAX_JSON_DEPTH {
        return Some(Value::String("[depth limit]".into()));
    }
    match value {
        Value::Object(object) if is_blob_object(object) => None,
        Value::Object(object) => {
            let mut clean = serde_json::Map::new();
            for (key, value) in object.iter().take(MAX_JSON_CONTAINER_ITEMS) {
                // Inline bytes/data are never useful analytical metadata.
                if matches!(key.as_str(), "data" | "bytes" | "base64") {
                    continue;
                }
                if let Some(value) = sanitize_record(value, depth + 1) {
                    clean.insert(key.clone(), value);
                }
            }
            Some(Value::Object(clean))
        }
        Value::Array(values) => Some(Value::Array(
            values
                .iter()
                .take(MAX_JSON_CONTAINER_ITEMS)
                .filter_map(|value| sanitize_record(value, depth + 1))
                .collect(),
        )),
        Value::String(string) => {
            if string.starts_with("data:") {
                None
            } else {
                Some(Value::String(
                    string.chars().take(MAX_JSON_STRING_BYTES).collect(),
                ))
            }
        }
        value => Some(value.clone()),
    }
}

fn compact_record_json(record: &Value, operation: &str) -> Option<String> {
    if operation == "delete" || record.is_null() {
        return None;
    }
    let clean = sanitize_record(record, 0)?;
    let encoded = serde_json::to_string(&clean).ok()?;
    if encoded.len() <= MAX_RECORD_JSON_BYTES {
        return Some(encoded);
    }
    let record_type = clean
        .get("$type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(serde_json::json!({"$type": record_type, "$truncated": true}).to_string())
}

pub fn normalize_text(s: &str) -> String {
    let lowered = s.to_lowercase();
    let url_re = Regex::new(r"https?://\S+").unwrap();
    let ws_re = Regex::new(r"\s+").unwrap();
    let no_urls = url_re.replace_all(&lowered, "<url>");
    ws_re.replace_all(no_urls.trim(), " ").into_owned()
}

pub fn simhash(text: &str) -> u64 {
    let normalized = normalize_text(text);
    let chars: Vec<char> = normalized.chars().collect();
    let tokens: Vec<String> = if chars.len() < 4 {
        vec![normalized]
    } else {
        chars.windows(4).map(|w| w.iter().collect()).collect()
    };
    let mut weights = [0i32; 64];
    for token in tokens {
        let h = xxh3_64(token.as_bytes());
        for (i, w) in weights.iter_mut().enumerate() {
            *w += if h & (1u64 << i) != 0 { 1 } else { -1 };
        }
    }
    weights.iter().enumerate().fold(
        0,
        |acc, (i, w)| if *w >= 0 { acc | (1u64 << i) } else { acc },
    )
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Extract the Jetstream resume position without requiring the event to be one
/// of the record types currently understood by the normalizer.
pub fn jetstream_time_us(raw: &[u8]) -> Result<i64> {
    let v: Value = serde_json::from_slice(raw).context("invalid Jetstream JSON")?;
    v.get("time_us")
        .and_then(Value::as_i64)
        .context("Jetstream event has no integer time_us")
}

pub fn parse_jetstream(raw: &[u8]) -> Result<Option<NormalizedEvent>> {
    let v: Value = serde_json::from_slice(raw).context("invalid Jetstream JSON")?;
    if v.get("kind").and_then(Value::as_str) != Some("commit") {
        return Ok(None);
    }
    let did = v["did"].as_str().unwrap_or_default().to_owned();
    let time_us = v["time_us"]
        .as_i64()
        .unwrap_or_else(|| Utc::now().timestamp_micros());
    let commit = &v["commit"];
    let operation = commit["operation"].as_str().unwrap_or_default();
    let collection = commit["collection"].as_str().unwrap_or_default().to_owned();
    let rkey = commit["rkey"].as_str().unwrap_or_default().to_owned();
    if did.is_empty() || collection.is_empty() || rkey.is_empty() || operation.is_empty() {
        anyhow::bail!("commit is missing did, operation, collection, or rkey");
    }
    let record = &commit["record"];
    let record_json = compact_record_json(record, operation);
    let at_uri = format!("at://{did}/{collection}/{rkey}");
    let mut kind = match collection.as_str() {
        "app.bsky.feed.post" => EventKind::Post,
        "app.bsky.feed.like" => EventKind::Like,
        "app.bsky.feed.repost" => EventKind::Repost,
        "app.bsky.graph.follow" => EventKind::Follow,
        _ => EventKind::Other,
    };
    if operation == "delete" {
        kind = if collection == "app.bsky.graph.follow" {
            EventKind::Unfollow
        } else {
            EventKind::Delete
        };
    }
    let subject = record
        .pointer("/subject/uri")
        .and_then(Value::as_str)
        .or_else(|| record.get("subject").and_then(Value::as_str))
        .map(str::to_owned);
    let reply_parent = record
        .pointer("/reply/parent/uri")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let reply_root = record
        .pointer("/reply/root/uri")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let quote_target = record
        .pointer("/embed/record/record/uri")
        .and_then(Value::as_str)
        .or_else(|| record.pointer("/embed/record/uri").and_then(Value::as_str))
        .map(str::to_owned);
    if matches!(kind, EventKind::Post) && reply_parent.is_some() {
        kind = EventKind::Reply;
    } else if matches!(kind, EventKind::Post) && quote_target.is_some() {
        kind = EventKind::Quote;
    }
    let text = record["text"].as_str().map(str::to_owned);
    let mut urls = HashSet::new();
    let mut domains = HashSet::new();
    if let Some(t) = &text {
        for word in t.split_whitespace() {
            if let Ok(u) = Url::parse(word.trim_matches(|c: char| ",.!?)]}".contains(c)))
                && matches!(u.scheme(), "http" | "https")
            {
                if let Some(d) = u.host_str() {
                    domains.insert(d.trim_start_matches("www.").to_owned());
                }
                urls.insert(u.to_string());
            }
        }
    }
    if let Some(features) = record.pointer("/facets").and_then(Value::as_array) {
        for facet in features {
            if let Some(fs) = facet["features"].as_array() {
                for f in fs {
                    if let Some(uri) = f["uri"].as_str()
                        && let Ok(u) = Url::parse(uri)
                    {
                        if let Some(d) = u.host_str() {
                            domains.insert(d.trim_start_matches("www.").to_owned());
                        }
                        urls.insert(uri.to_owned());
                    }
                }
            }
        }
    }
    let target = subject
        .clone()
        .or_else(|| reply_parent.clone())
        .or_else(|| quote_target.clone());
    let target_did = if collection == "app.bsky.graph.follow" {
        record["subject"].as_str().map(str::to_owned)
    } else {
        target
            .as_deref()
            .and_then(|s| s.strip_prefix("at://"))
            .and_then(|s| s.split('/').next())
            .map(str::to_owned)
    };
    // `createdAt` is supplied by the repository client and is neither ordered nor
    // trustworthy (old drafts and future-dated spam are both common).  The
    // Jetstream envelope time is the observation time and must drive live
    // windows, ordering, and retention; otherwise one future timestamp can evict
    // the entire hot buffer.
    let timestamp = DateTime::from_timestamp_micros(time_us).unwrap_or_else(Utc::now);
    let normalized = text.as_deref().map(normalize_text);
    Ok(Some(NormalizedEvent {
        event_id: format!("{did}:{collection}:{rkey}:{operation}:{time_us}"),
        time_us,
        timestamp,
        actor_did: did,
        kind,
        collection,
        rkey,
        operation: operation.to_owned(),
        at_uri,
        record_json,
        target,
        target_did,
        text_hash: text.as_deref().map(|x| xxh3_64(x.as_bytes())),
        normalized_text_hash: normalized.as_deref().map(|x| xxh3_64(x.as_bytes())),
        simhash: text.as_deref().map(simhash),
        text,
        reply_parent,
        reply_root,
        quote_target,
        urls: urls.into_iter().collect(),
        domains: domains.into_iter().collect(),
        languages: record["langs"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprints_normalize_urls_and_case() {
        assert_eq!(normalize_text("  HELLO   https://x.test/a "), "hello <url>");
        assert_eq!(simhash("Same message"), simhash("same   message"));
        assert!(
            hamming(
                simhash("identical campaign text"),
                simhash("identical campaign texts")
            ) < 20
        );
    }
    #[test]
    fn parses_post() {
        let raw=br#"{"did":"did:plc:a","time_us":1700000000000000,"kind":"commit","commit":{"operation":"create","collection":"app.bsky.feed.post","rkey":"x","record":{"$type":"app.bsky.feed.post","text":"hi https://example.com/x","langs":["en"],"createdAt":"2023-11-14T22:13:20Z"}}}"#;
        let e = parse_jetstream(raw).unwrap().unwrap();
        assert_eq!(e.actor_did, "did:plc:a");
        assert_eq!(e.domains, vec!["example.com"]);
        assert_eq!(e.languages, vec!["en"]);
        assert_eq!(e.timestamp.timestamp_micros(), 1_700_000_000_000_000);
    }
    #[test]
    fn client_created_at_cannot_poison_live_event_time() {
        let raw=br#"{"did":"did:plc:a","time_us":1700000000000000,"kind":"commit","commit":{"operation":"create","collection":"app.bsky.feed.post","rkey":"x","record":{"text":"hi","createdAt":"2030-01-01T00:00:00Z"}}}"#;
        let event = parse_jetstream(raw).unwrap().unwrap();
        assert_eq!(event.timestamp.timestamp_micros(), 1_700_000_000_000_000);
    }
    #[test]
    fn arbitrary_lexicon_is_retained_without_blobs() {
        let raw=br#"{"did":"did:plc:a","time_us":1700000000000000,"kind":"commit","commit":{"operation":"create","collection":"com.example.recipe","rkey":"x","record":{"$type":"com.example.recipe","title":"Soup","photo":{"$type":"blob","ref":{"$link":"bafk-secret"},"mimeType":"image/jpeg","size":123},"nested":{"images":[{"cid":"legacy","mimeType":"image/png"}],"safe":true}}}}"#;
        let event = parse_jetstream(raw).unwrap().unwrap();
        assert_eq!(event.kind, EventKind::Other);
        assert_eq!(event.operation, "create");
        let json = event.record_json.unwrap();
        assert!(json.contains("Soup"));
        assert!(json.contains("safe"));
        assert!(!json.contains("bafk-secret"));
        assert!(!json.contains("legacy"));
    }
    #[test]
    fn generic_record_json_is_bounded_and_delete_has_none() {
        let huge = "x".repeat(MAX_RECORD_JSON_BYTES * 2);
        let raw = serde_json::json!({"did":"did:plc:a","time_us":1700000000000000_i64,"kind":"commit","commit":{"operation":"create","collection":"org.test.large","rkey":"x","record":{"$type":"org.test.large","value":huge}}});
        let event = parse_jetstream(raw.to_string().as_bytes())
            .unwrap()
            .unwrap();
        assert!(event.record_json.unwrap().len() <= MAX_RECORD_JSON_BYTES);
        let delete = br#"{"did":"did:plc:a","time_us":1700000000000001,"kind":"commit","commit":{"operation":"delete","collection":"org.test.large","rkey":"x"}}"#;
        let event = parse_jetstream(delete).unwrap().unwrap();
        assert_eq!(event.kind, EventKind::Delete);
        assert_eq!(event.operation, "delete");
        assert!(event.record_json.is_none());
    }
}
