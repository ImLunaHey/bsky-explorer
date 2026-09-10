use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Post,
    Like,
    Repost,
    Follow,
    Unfollow,
    Reply,
    Quote,
    Delete,
    Other,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Post => "post",
            Self::Like => "like",
            Self::Repost => "repost",
            Self::Follow => "follow",
            Self::Unfollow => "unfollow",
            Self::Reply => "reply",
            Self::Quote => "quote",
            Self::Delete => "delete",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedEvent {
    pub event_id: String,
    pub time_us: i64,
    pub timestamp: DateTime<Utc>,
    pub actor_did: String,
    pub kind: EventKind,
    pub collection: String,
    pub rkey: String,
    pub at_uri: String,
    pub target: Option<String>,
    pub target_did: Option<String>,
    pub text: Option<String>,
    pub reply_parent: Option<String>,
    pub reply_root: Option<String>,
    pub quote_target: Option<String>,
    pub urls: Vec<String>,
    pub domains: Vec<String>,
    pub languages: Vec<String>,
    pub text_hash: Option<u64>,
    pub normalized_text_hash: Option<u64>,
    pub simhash: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub automation_score: f64,
    pub coordination_score: f64,
    pub shared_target_score: f64,
    pub timing_similarity_score: f64,
    pub content_similarity_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceEvent {
    pub event_id: String,
    pub actor_did: String,
    pub kind: String,
    pub timestamp: DateTime<Utc>,
    pub target: Option<String>,
    pub at_uri: Option<String>,
    pub text_excerpt: Option<String>,
    pub reply_parent: Option<String>,
    pub reply_root: Option<String>,
    pub quote_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub cluster_id: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub accounts: Vec<String>,
    pub event_count: usize,
    pub scores: ScoreBreakdown,
    pub shared_target_ratio: f64,
    pub median_timing_difference_ms: f64,
    pub repeated_content_count: usize,
    pub common_urls: Vec<String>,
    pub common_domains: Vec<String>,
    pub common_targets: Vec<String>,
    pub sample_events: Vec<EvidenceEvent>,
}
