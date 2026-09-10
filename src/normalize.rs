use crate::model::{EventKind, NormalizedEvent};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use url::Url;
use xxhash_rust::xxh3::xxh3_64;

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
        at_uri,
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
}
