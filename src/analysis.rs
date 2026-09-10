use crate::model::{Detection, EventKind, EvidenceEvent, NormalizedEvent, ScoreBreakdown};
use chrono::{Duration, Utc};
use parking_lot::RwLock;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, Serialize)]
pub struct Rates {
    pub events_per_sec: f64,
    pub posts_per_sec: f64,
    pub likes_per_sec: f64,
    pub follows_per_sec: f64,
    pub interactions_per_sec: f64,
    /// Rates for every configured sliding window. The legacy top-level fields
    /// remain the 60-second (or nearest configured) view.
    pub windows: Vec<WindowRates>,
}
#[derive(Clone, Serialize)]
pub struct WindowRates {
    pub seconds: u64,
    pub events_per_sec: f64,
    pub posts_per_sec: f64,
    pub likes_per_sec: f64,
    pub follows_per_sec: f64,
    pub interactions_per_sec: f64,
}
#[derive(Clone, Serialize)]
pub struct AccountSummary {
    pub did: String,
    pub events_24h: usize,
    pub posts_24h: usize,
    pub unique_targets: usize,
    pub automation_score: f64,
}
#[derive(Clone, Serialize)]
pub struct AccountWindowActivity {
    pub seconds: u64,
    pub events: usize,
    pub posts: usize,
    pub likes: usize,
    pub reposts: usize,
    pub follows: usize,
    pub interactions: usize,
    pub events_per_second: f64,
}
#[derive(Clone, Serialize)]
pub struct AccountActivity {
    #[serde(flatten)]
    pub summary: AccountSummary,
    pub first_seen: Option<chrono::DateTime<Utc>>,
    pub last_seen: Option<chrono::DateTime<Utc>>,
    pub windows: Vec<AccountWindowActivity>,
    pub unique_domains: Vec<String>,
    pub unique_urls: Vec<String>,
    pub recent_events: Vec<NormalizedEvent>,
    pub cluster_ids: Vec<String>,
    pub scores: ScoreBreakdown,
}
#[derive(Clone, Serialize)]
pub struct TrendingItem {
    pub key: String,
    pub count: u64,
    pub recent_count: u64,
    pub growth: i64,
    pub sample: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct CategorizedAnomalies {
    pub coordination_clusters: Vec<Detection>,
    pub unusual_posting: Vec<AccountSummary>,
    pub repeated_content_campaigns: Vec<Detection>,
}
#[derive(Clone, Serialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub event_count: usize,
}
#[derive(Clone, Serialize)]
pub struct TimelineBucket {
    pub timestamp: chrono::DateTime<Utc>,
    pub event_count: usize,
}
#[derive(Clone, Serialize)]
pub struct ClusterDetail {
    #[serde(flatten)]
    pub detection: Detection,
    pub timeline: Vec<TimelineBucket>,
    pub graph_edges: Vec<GraphEdge>,
}
#[derive(Clone, Default)]
pub struct RecentEventFilter {
    pub after_time_us: Option<i64>,
    pub kind: Option<String>,
    pub query: Option<String>,
    pub limit: usize,
}
#[derive(Clone, Serialize)]
pub struct RecentEvents {
    pub events: Vec<NormalizedEvent>,
    pub next_after_time_us: Option<i64>,
    pub retained_event_count: usize,
    pub has_more: bool,
}
#[derive(Clone, Copy)]
pub enum TrendDimension {
    Domain,
    Url,
    Fingerprint(u64),
}
#[derive(Clone, Serialize)]
pub struct TrendDrilldown {
    pub events: Vec<NormalizedEvent>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub cluster_ids: Vec<String>,
}
#[derive(Clone)]
pub struct Analyzer {
    inner: Arc<RwLock<State>>,
    coordination_secs: i64,
    min_accounts: usize,
    min_events: usize,
    retention_hours: i64,
    windows_seconds: Arc<Vec<u64>>,
}
struct State {
    events: VecDeque<NormalizedEvent>,
    /// Highest event timestamp observed. Retention follows event time so that
    /// archives can be replayed without every historical event being evicted.
    watermark: Option<chrono::DateTime<Utc>>,
    detections: HashMap<String, Detection>,
    url_counts: HashMap<String, u64>,
    domain_counts: HashMap<String, u64>,
}
impl Analyzer {
    pub fn new(
        coordination_secs: i64,
        min_accounts: usize,
        min_events: usize,
        retention_hours: i64,
    ) -> Self {
        Self::with_windows(
            coordination_secs,
            min_accounts,
            min_events,
            retention_hours,
            vec![60, 300, 3600, 86400],
        )
    }
    pub fn with_windows(
        coordination_secs: i64,
        min_accounts: usize,
        min_events: usize,
        retention_hours: i64,
        mut windows_seconds: Vec<u64>,
    ) -> Self {
        windows_seconds.retain(|seconds| *seconds > 0);
        windows_seconds.sort_unstable();
        windows_seconds.dedup();
        if windows_seconds.is_empty() {
            windows_seconds.push(60);
        }
        Self {
            inner: Arc::new(RwLock::new(State {
                events: VecDeque::new(),
                watermark: None,
                detections: HashMap::new(),
                url_counts: HashMap::new(),
                domain_counts: HashMap::new(),
            })),
            coordination_secs,
            min_accounts,
            min_events,
            retention_hours,
            windows_seconds: Arc::new(windows_seconds),
        }
    }
    pub fn observe(&self, e: NormalizedEvent) -> Option<Detection> {
        self.observe_inner(e, true)
    }
    pub fn observe_without_detection(&self, e: NormalizedEvent) {
        self.observe_inner(e, false);
    }
    fn observe_inner(&self, e: NormalizedEvent, detect: bool) -> Option<Detection> {
        let mut s = self.inner.write();
        for x in &e.urls {
            *s.url_counts.entry(x.clone()).or_default() += 1
        }
        for x in &e.domains {
            *s.domain_counts.entry(x.clone()).or_default() += 1
        }
        let ordered = s.watermark.is_none_or(|watermark| e.timestamp >= watermark);
        s.events.push_back(e.clone());
        s.watermark = Some(s.watermark.map_or(e.timestamp, |w| w.max(e.timestamp)));
        let cutoff = s.watermark.unwrap() - Duration::hours(self.retention_hours);
        if ordered {
            while s
                .events
                .front()
                .is_some_and(|event| event.timestamp < cutoff)
            {
                s.events.pop_front();
            }
        } else {
            // Replay may be out of order; keep its slower correctness path away
            // from the latency-sensitive ordered firehose.
            s.events.retain(|event| event.timestamp >= cutoff);
        }
        if !detect {
            return None;
        }
        let key = e
            .target
            .clone()
            .or_else(|| e.normalized_text_hash.map(|h| format!("text:{h}")))
            .or_else(|| e.domains.first().map(|d| format!("domain:{d}")))?;
        let candidates: Box<dyn Iterator<Item = &NormalizedEvent>> = if ordered {
            Box::new(s.events.iter().rev().take_while(|event| {
                e.timestamp
                    .signed_duration_since(event.timestamp)
                    .num_seconds()
                    <= self.coordination_secs
            }))
        } else {
            Box::new(s.events.iter())
        };
        let near: Vec<_> = candidates
            .filter(|x| {
                x.actor_did != e.actor_did
                    && (x.timestamp - e.timestamp).num_seconds().abs() <= self.coordination_secs
                    && (x.target.as_ref() == e.target.as_ref() && e.target.is_some()
                        || x.normalized_text_hash.map(|h| format!("text:{h}")) == Some(key.clone())
                        || near_simhash(x.simhash, e.simhash)
                        || (!e.urls.is_empty() && x.urls.iter().any(|u| e.urls.contains(u)))
                        || x.domains.iter().any(|d| key == format!("domain:{d}")))
            })
            .cloned()
            .chain(std::iter::once(e.clone()))
            .collect();
        let accounts: HashSet<_> = near.iter().map(|x| x.actor_did.clone()).collect();
        if accounts.len() < self.min_accounts || near.len() < self.min_events {
            return None;
        }
        let mut accounts: Vec<_> = accounts.into_iter().collect();
        accounts.sort();
        let cluster_id = format!(
            "coord-{:016x}",
            xxhash_rust::xxh3::xxh3_64(format!("{}:{}", key, accounts.join(",")).as_bytes())
        );
        let first = near.iter().map(|x| x.timestamp).min().unwrap();
        let last = near.iter().map(|x| x.timestamp).max().unwrap();
        let repeat = near
            .iter()
            .filter(|x| x.normalized_text_hash.is_some())
            .count();
        let timing = median_timing_difference_ms(&near);
        let target_ratio = dominant_ratio(near.iter().filter_map(|x| x.target.as_deref()));
        let content = content_similarity(&near);
        let timing_score =
            (1.0 - timing / (self.coordination_secs as f64 * 1000.0)).clamp(0.0, 1.0);
        let coordination =
            (0.45 * target_ratio + 0.3 * timing_score + 0.25 * content).clamp(0.0, 1.0);
        let urls = counts(near.iter().flat_map(|x| x.urls.clone()), 5);
        let domains = counts(near.iter().flat_map(|x| x.domains.clone()), 5);
        let targets = counts(near.iter().filter_map(|x| x.target.clone()), 5);
        let d = Detection {
            cluster_id: cluster_id.clone(),
            first_seen: first,
            last_seen: last,
            accounts,
            event_count: near.len(),
            scores: ScoreBreakdown {
                automation_score: 0.0,
                coordination_score: coordination,
                shared_target_score: target_ratio,
                timing_similarity_score: timing_score,
                content_similarity_score: content,
            },
            shared_target_ratio: target_ratio,
            median_timing_difference_ms: timing,
            repeated_content_count: repeat,
            common_urls: urls,
            common_domains: domains,
            common_targets: targets,
            sample_events: near
                .iter()
                .take(12)
                .map(|x| EvidenceEvent {
                    event_id: x.event_id.clone(),
                    actor_did: x.actor_did.clone(),
                    kind: x.kind.as_str().into(),
                    timestamp: x.timestamp,
                    target: x.target.clone(),
                    at_uri: (!x.at_uri.is_empty()).then(|| x.at_uri.clone()),
                    text_excerpt: x.text.as_deref().map(truncate_sample),
                    reply_parent: x.reply_parent.clone(),
                    reply_root: x.reply_root.clone(),
                    quote_target: x.quote_target.clone(),
                })
                .collect(),
        };
        s.detections.insert(cluster_id, d.clone());
        Some(d)
    }

    /// Returns a bounded, chronological view over the already-retained hot
    /// analysis window. This deliberately does not create a second firehose
    /// buffer, so its memory bound remains the configured analyzer retention.
    pub fn recent_events(&self, filter: RecentEventFilter) -> RecentEvents {
        const MAX_LIMIT: usize = 500;
        let s = self.inner.read();
        let limit = filter.limit.clamp(1, MAX_LIMIT);
        let query = filter
            .query
            .as_deref()
            .map(str::trim)
            .filter(|query| !query.is_empty())
            .map(str::to_lowercase);
        let kind = filter
            .kind
            .as_deref()
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
            .map(str::to_lowercase);

        let matches = |event: &&NormalizedEvent| {
            filter
                .after_time_us
                .is_none_or(|after| event.time_us > after)
                && kind
                    .as_deref()
                    .is_none_or(|kind| event.kind.as_str() == kind)
                && query
                    .as_deref()
                    .is_none_or(|query| event_matches(event, query))
        };
        let matching_count = s.events.iter().filter(matches).count();
        let skip = matching_count.saturating_sub(limit);
        let events = s
            .events
            .iter()
            .filter(matches)
            .skip(skip)
            .cloned()
            .collect();
        RecentEvents {
            events,
            // Advance over unrelated events as well; otherwise a selective
            // poll repeatedly scans the same portion of the retained window.
            next_after_time_us: s.events.back().map(|event| event.time_us),
            retained_event_count: s.events.len(),
            has_more: matching_count > limit,
        }
    }
    pub fn trend_drilldown(
        &self,
        dimension: TrendDimension,
        key: &str,
        kind: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> TrendDrilldown {
        let s = self.inner.read();
        let matches_dimension = |event: &&NormalizedEvent| match dimension {
            TrendDimension::Domain => event
                .domains
                .iter()
                .any(|domain| domain.eq_ignore_ascii_case(key)),
            TrendDimension::Url => event.urls.iter().any(|url| url == key),
            TrendDimension::Fingerprint(hash) => event.normalized_text_hash == Some(hash),
        };
        let matches_kind = |event: &&NormalizedEvent| {
            kind.is_none_or(|kind| event.kind.as_str().eq_ignore_ascii_case(kind))
        };
        let matching: Vec<_> = s
            .events
            .iter()
            .rev()
            .filter(matches_dimension)
            .filter(matches_kind)
            .collect();
        let event_ids: HashSet<_> = matching.iter().map(|event| &event.event_id).collect();
        let mut cluster_ids: Vec<_> = s
            .detections
            .values()
            .filter(|detection| {
                detection
                    .sample_events
                    .iter()
                    .any(|event| event_ids.contains(&event.event_id))
            })
            .map(|detection| detection.cluster_id.clone())
            .collect();
        cluster_ids.sort();
        TrendDrilldown {
            total: matching.len(),
            events: matching
                .into_iter()
                .skip(offset)
                .take(limit)
                .cloned()
                .collect(),
            offset,
            limit,
            cluster_ids,
        }
    }
    pub fn retained_record(&self, uri: &str) -> Option<NormalizedEvent> {
        self.inner
            .read()
            .events
            .iter()
            .rev()
            .find(|event| {
                event.at_uri == uri
                    && matches!(
                        event.kind,
                        EventKind::Post | EventKind::Reply | EventKind::Quote
                    )
            })
            .cloned()
    }
    pub fn rates(&self) -> Rates {
        let s = self.inner.read();
        // During cursor recovery the watermark can trail wall time. Rates should
        // describe the stream currently being processed instead of displaying
        // misleading zeros until catch-up completes.
        let now = s.watermark.unwrap_or_else(Utc::now);
        let windows: Vec<_> = self
            .windows_seconds
            .iter()
            .map(|seconds| window_rates(&s.events, now, *seconds))
            .collect();
        let primary = windows
            .iter()
            .min_by_key(|window| window.seconds.abs_diff(60))
            .unwrap();
        Rates {
            events_per_sec: primary.events_per_sec,
            posts_per_sec: primary.posts_per_sec,
            likes_per_sec: primary.likes_per_sec,
            follows_per_sec: primary.follows_per_sec,
            interactions_per_sec: primary.interactions_per_sec,
            windows,
        }
    }
    pub fn detections(&self) -> Vec<Detection> {
        self.inner.read().detections.values().cloned().collect()
    }
    pub fn detection(&self, id: &str) -> Option<Detection> {
        self.inner.read().detections.get(id).cloned()
    }
    pub fn cluster_detail(&self, id: &str) -> Option<ClusterDetail> {
        let s = self.inner.read();
        let detection = s.detections.get(id)?.clone();
        let evidence_ids: HashSet<_> = detection
            .sample_events
            .iter()
            .map(|e| &e.event_id)
            .collect();
        let events: Vec<_> = s
            .events
            .iter()
            .filter(|e| evidence_ids.contains(&e.event_id))
            .collect();
        let mut timeline = HashMap::new();
        let mut edges = HashMap::new();
        for event in events {
            let seconds = event.timestamp.timestamp();
            let bucket = chrono::DateTime::from_timestamp(seconds - seconds.rem_euclid(60), 0)
                .unwrap_or(event.timestamp);
            *timeline.entry(bucket).or_insert(0usize) += 1;
            if let Some(target) = event.target_did.as_ref().or(event.target.as_ref()) {
                *edges
                    .entry((event.actor_did.clone(), target.clone()))
                    .or_insert(0usize) += 1;
            }
        }
        let mut timeline: Vec<_> = timeline
            .into_iter()
            .map(|(timestamp, event_count)| TimelineBucket {
                timestamp,
                event_count,
            })
            .collect();
        timeline.sort_by_key(|b| b.timestamp);
        let mut graph_edges: Vec<_> = edges
            .into_iter()
            .map(|((source, target), event_count)| GraphEdge {
                source,
                target,
                event_count,
            })
            .collect();
        graph_edges.sort_by_key(|e| std::cmp::Reverse(e.event_count));
        Some(ClusterDetail {
            detection,
            timeline,
            graph_edges,
        })
    }
    pub fn trending_fingerprints(&self) -> Vec<TrendingItem> {
        let s = self.inner.read();
        let end = s.watermark.unwrap_or_else(Utc::now);
        let cutoff = end - Duration::hours(1);
        let mut grouped: HashMap<u64, (u64, Option<String>)> = HashMap::new();
        for event in s.events.iter().filter(|e| e.timestamp >= cutoff) {
            if let Some(hash) = event.normalized_text_hash {
                let item = grouped.entry(hash).or_default();
                item.0 += 1;
                if item.1.is_none() {
                    item.1 = event.text.as_deref().map(truncate_sample);
                }
            }
        }
        let mut result: Vec<_> = grouped
            .into_iter()
            .map(|(hash, (count, sample))| TrendingItem {
                key: format!("{hash:016x}"),
                count,
                recent_count: count,
                growth: count as i64,
                sample,
            })
            .collect();
        result.sort_by_key(|x| std::cmp::Reverse(x.count));
        result.truncate(20);
        result
    }
    pub fn growing_conversations(&self) -> Vec<TrendingItem> {
        let s = self.inner.read();
        let end = s.watermark.unwrap_or_else(Utc::now);
        let recent = end - Duration::minutes(5);
        let previous = end - Duration::minutes(10);
        let mut grouped: HashMap<String, (u64, u64)> = HashMap::new();
        for event in s.events.iter().filter(|e| e.timestamp >= previous) {
            let key = event.reply_root.as_ref().or(event.reply_parent.as_ref());
            if let Some(key) = key {
                let counts = grouped.entry(key.clone()).or_default();
                if event.timestamp >= recent {
                    counts.0 += 1
                } else {
                    counts.1 += 1
                }
            }
        }
        let mut result: Vec<_> = grouped
            .into_iter()
            .map(|(key, (now, before))| TrendingItem {
                key,
                count: now + before,
                recent_count: now,
                growth: now as i64 - before as i64,
                sample: None,
            })
            .filter(|x| x.recent_count > 0)
            .collect();
        result.sort_by_key(|x| {
            (
                std::cmp::Reverse(x.growth),
                std::cmp::Reverse(x.recent_count),
            )
        });
        result.truncate(20);
        result
    }
    pub fn categorized_anomalies(&self) -> CategorizedAnomalies {
        let s = self.inner.read();
        let mut dids: HashSet<_> = s.events.iter().map(|e| e.actor_did.clone()).collect();
        let mut unusual_posting: Vec<_> = dids
            .drain()
            .map(|did| self.account_from_state(&s, &did))
            .filter(|a| a.automation_score >= 0.65)
            .collect();
        unusual_posting.sort_by(|a, b| b.automation_score.total_cmp(&a.automation_score));
        let mut coordination_clusters: Vec<_> = s.detections.values().cloned().collect();
        coordination_clusters.sort_by(|a, b| {
            b.scores
                .coordination_score
                .total_cmp(&a.scores.coordination_score)
        });
        let repeated_content_campaigns = coordination_clusters
            .iter()
            .filter(|d| {
                d.repeated_content_count >= self.min_events
                    && d.scores.content_similarity_score >= 0.5
            })
            .cloned()
            .collect();
        CategorizedAnomalies {
            coordination_clusters,
            unusual_posting,
            repeated_content_campaigns,
        }
    }
    pub fn top(&self, domains: bool) -> Vec<(String, u64)> {
        let s = self.inner.read();
        let m = if domains {
            &s.domain_counts
        } else {
            &s.url_counts
        };
        let mut v: Vec<_> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        v.truncate(20);
        v
    }
    pub fn account(&self, did: &str) -> AccountSummary {
        let s = self.inner.read();
        self.account_from_state(&s, did)
    }
    pub fn account_activity(&self, did: &str) -> AccountActivity {
        let s = self.inner.read();
        let account_events: Vec<_> = s.events.iter().filter(|e| e.actor_did == did).collect();
        let now = s.watermark.unwrap_or_else(Utc::now);
        let mut domains: HashSet<_> = account_events
            .iter()
            .flat_map(|e| e.domains.clone())
            .collect();
        let mut urls: HashSet<_> = account_events.iter().flat_map(|e| e.urls.clone()).collect();
        let mut unique_domains: Vec<_> = domains.drain().collect();
        let mut unique_urls: Vec<_> = urls.drain().collect();
        unique_domains.sort();
        unique_urls.sort();
        unique_domains.truncate(50);
        unique_urls.truncate(50);
        let windows = self
            .windows_seconds
            .iter()
            .map(|seconds| {
                let cutoff = now - Duration::seconds(*seconds as i64);
                let events: Vec<_> = account_events
                    .iter()
                    .filter(|e| e.timestamp >= cutoff)
                    .collect();
                let count = |kind: &str| events.iter().filter(|e| e.kind.as_str() == kind).count();
                AccountWindowActivity {
                    seconds: *seconds,
                    events: events.len(),
                    posts: count("post") + count("reply") + count("quote"),
                    likes: count("like"),
                    reposts: count("repost"),
                    follows: count("follow"),
                    interactions: events
                        .iter()
                        .filter(|e| !matches!(e.kind, crate::model::EventKind::Post))
                        .count(),
                    events_per_second: events.len() as f64 / *seconds as f64,
                }
            })
            .collect();
        let mut recent_events: Vec<_> = account_events
            .iter()
            .rev()
            .take(50)
            .map(|e| (*e).clone())
            .collect();
        recent_events.sort_by_key(|e| std::cmp::Reverse(e.timestamp));
        let mut cluster_ids: Vec<_> = s
            .detections
            .values()
            .filter(|d| d.accounts.iter().any(|account| account == did))
            .map(|d| d.cluster_id.clone())
            .collect();
        cluster_ids.sort();
        let summary = self.account_from_state(&s, did);
        let member_detections: Vec<_> = s
            .detections
            .values()
            .filter(|d| d.accounts.iter().any(|account| account == did))
            .collect();
        let max_score = |score: fn(&Detection) -> f64| {
            member_detections
                .iter()
                .map(|d| score(d))
                .fold(0.0, f64::max)
        };
        AccountActivity {
            scores: ScoreBreakdown {
                automation_score: summary.automation_score,
                coordination_score: max_score(|d| d.scores.coordination_score),
                shared_target_score: max_score(|d| d.scores.shared_target_score),
                timing_similarity_score: max_score(|d| d.scores.timing_similarity_score),
                content_similarity_score: max_score(|d| d.scores.content_similarity_score),
            },
            summary,
            first_seen: account_events.iter().map(|e| e.timestamp).min(),
            last_seen: account_events.iter().map(|e| e.timestamp).max(),
            windows,
            unique_domains,
            unique_urls,
            recent_events,
            cluster_ids,
        }
    }
    fn account_from_state(&self, s: &State, did: &str) -> AccountSummary {
        let cutoff = s.watermark.unwrap_or_else(Utc::now) - Duration::hours(24);
        let e: Vec<_> = s
            .events
            .iter()
            .filter(|e| e.actor_did == did && e.timestamp >= cutoff)
            .collect();
        let posts = e
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    crate::model::EventKind::Post
                        | crate::model::EventKind::Reply
                        | crate::model::EventKind::Quote
                )
            })
            .count();
        let targets: HashSet<_> = e.iter().filter_map(|e| e.target.clone()).collect();
        let mut timestamps: Vec<_> = e.iter().map(|e| e.timestamp).collect();
        timestamps.sort_unstable();
        let mut intervals: Vec<_> = timestamps
            .windows(2)
            .map(|w| (w[1] - w[0]).num_seconds())
            .collect();
        let regularity = if intervals.len() > 3 {
            let mean = intervals.iter().sum::<i64>() as f64 / intervals.len() as f64;
            let variance = intervals
                .drain(..)
                .map(|x| (x as f64 - mean).powi(2))
                .sum::<f64>()
                / e.len() as f64;
            (1.0 - variance.sqrt() / (mean.abs() + 1.0)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let volume = (e.len() as f64 / 500.0).min(1.0);
        let active_hours: HashSet<_> = e
            .iter()
            .map(|event| event.timestamp.format("%Y-%m-%dT%H").to_string())
            .collect();
        // Require enough samples as well as broad hour coverage: two isolated
        // night-time posts should not look like sustained round-the-clock use.
        let coverage =
            (active_hours.len() as f64 / 24.0).min(1.0) * (e.len() as f64 / 24.0).min(1.0);
        AccountSummary {
            did: did.into(),
            events_24h: e.len(),
            posts_24h: posts,
            unique_targets: targets.len(),
            automation_score: 0.5 * volume + 0.3 * regularity + 0.2 * coverage,
        }
    }
}
fn truncate_sample(text: &str) -> String {
    text.chars().take(160).collect()
}
fn window_rates(
    events: &VecDeque<NormalizedEvent>,
    now: chrono::DateTime<Utc>,
    seconds: u64,
) -> WindowRates {
    let cutoff = now - Duration::seconds(seconds.min(i64::MAX as u64) as i64);
    let xs: Vec<_> = events
        .iter()
        .filter(|event| event.timestamp >= cutoff)
        .collect();
    let divisor = seconds as f64;
    let count = |kind: &str| xs.iter().filter(|e| e.kind.as_str() == kind).count() as f64;
    WindowRates {
        seconds,
        events_per_sec: xs.len() as f64 / divisor,
        posts_per_sec: (count("post") + count("reply") + count("quote")) / divisor,
        likes_per_sec: count("like") / divisor,
        follows_per_sec: count("follow") / divisor,
        interactions_per_sec: xs
            .iter()
            .filter(|e| !matches!(e.kind, crate::model::EventKind::Post))
            .count() as f64
            / divisor,
    }
}
fn near_simhash(a: Option<u64>, b: Option<u64>) -> bool {
    matches!((a, b), (Some(a), Some(b)) if (a ^ b).count_ones() <= 3)
}

fn dominant_ratio<'a>(items: impl Iterator<Item = &'a str>) -> f64 {
    let mut counts = HashMap::new();
    let mut total = 0usize;
    for item in items {
        *counts.entry(item).or_insert(0usize) += 1;
        total += 1;
    }
    counts.values().copied().max().unwrap_or(0) as f64 / total.max(1) as f64
}

fn content_similarity(events: &[NormalizedEvent]) -> f64 {
    let with_content: Vec<_> = events
        .iter()
        .filter(|e| e.normalized_text_hash.is_some() || e.simhash.is_some())
        .collect();
    if with_content.is_empty() {
        return 0.0;
    }
    let best = with_content
        .iter()
        .map(|candidate| {
            with_content
                .iter()
                .filter(|other| {
                    candidate.normalized_text_hash == other.normalized_text_hash
                        || near_simhash(candidate.simhash, other.simhash)
                })
                .count()
        })
        .max()
        .unwrap_or(0);
    best as f64 / with_content.len() as f64
}

fn median_timing_difference_ms(events: &[NormalizedEvent]) -> f64 {
    let mut timestamps: Vec<_> = events.iter().map(|e| e.timestamp).collect();
    timestamps.sort_unstable();
    let mut gaps: Vec<_> = timestamps
        .windows(2)
        .map(|w| (w[1] - w[0]).num_milliseconds().unsigned_abs())
        .collect();
    if gaps.is_empty() {
        return 0.0;
    }
    gaps.sort_unstable();
    let mid = gaps.len() / 2;
    if gaps.len() % 2 == 0 {
        (gaps[mid - 1] + gaps[mid]) as f64 / 2.0
    } else {
        gaps[mid] as f64
    }
}
fn counts<I: Iterator<Item = String>>(it: I, n: usize) -> Vec<String> {
    let mut m = HashMap::new();
    for x in it {
        *m.entry(x).or_insert(0usize) += 1
    }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.into_iter().take(n).map(|x| x.0).collect()
}

fn event_matches(event: &NormalizedEvent, query: &str) -> bool {
    let contains = |value: &str| value.to_lowercase().contains(query);
    contains(&event.actor_did)
        || contains(&event.at_uri)
        || event.text.as_deref().is_some_and(contains)
        || event.target.as_deref().is_some_and(contains)
        || event.target_did.as_deref().is_some_and(contains)
        || event.reply_parent.as_deref().is_some_and(contains)
        || event.reply_root.as_deref().is_some_and(contains)
        || event.quote_target.as_deref().is_some_and(contains)
        || event.urls.iter().any(|value| contains(value))
        || event.domains.iter().any(|value| contains(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EventKind;
    fn e(actor: &str, t: i64) -> NormalizedEvent {
        NormalizedEvent {
            event_id: format!("{actor}{t}"),
            time_us: t * 1_000_000,
            timestamp: chrono::DateTime::from_timestamp(t, 0).unwrap(),
            actor_did: actor.into(),
            kind: EventKind::Repost,
            collection: "x".into(),
            rkey: "r".into(),
            at_uri: "u".into(),
            target: Some("at://target".into()),
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
        }
    }
    #[test]
    fn coordinated_group_scores() {
        let a = Analyzer::new(30, 3, 3, 24);
        assert!(a.observe(e("a", 100)).is_none());
        assert!(a.observe(e("b", 101)).is_none());
        let d = a.observe(e("c", 102)).unwrap();
        assert_eq!(d.accounts.len(), 3);
        assert!(d.scores.coordination_score > 0.6);
        assert_eq!(d.sample_events[0].at_uri.as_deref(), Some("u"));
    }
    #[test]
    fn detection_evidence_keeps_public_record_context_without_blobs() {
        let a = Analyzer::new(30, 2, 2, 24);
        let first = e("alice", 100);
        a.observe(first);
        let mut second = e("bob", 101);
        second.text = Some("A useful excerpt for investigators".into());
        second.reply_parent = Some("at://did:plc:parent/app.bsky.feed.post/one".into());
        second.reply_root = Some("at://did:plc:root/app.bsky.feed.post/two".into());
        second.quote_target = Some("at://did:plc:quoted/app.bsky.feed.post/three".into());
        let detection = a.observe(second).expect("coordinated pair");
        let sample = detection
            .sample_events
            .iter()
            .find(|event| event.actor_did == "bob")
            .unwrap();
        assert_eq!(sample.at_uri.as_deref(), Some("u"));
        assert_eq!(
            sample.text_excerpt.as_deref(),
            Some("A useful excerpt for investigators")
        );
        assert!(sample.reply_parent.as_deref().unwrap().contains("parent"));
        assert!(sample.reply_root.as_deref().unwrap().contains("root"));
        assert!(sample.quote_target.as_deref().unwrap().contains("quoted"));
        let json = serde_json::to_string(sample).unwrap();
        for forbidden in ["blob", "avatar", "banner", "image", "video"] {
            assert!(!json.contains(forbidden));
        }
    }
    #[test]
    fn account_activity_exposes_windows_evidence_and_clusters() {
        let a = Analyzer::with_windows(30, 3, 3, 24, vec![60, 300]);
        for (actor, time) in [("alice", 100), ("bob", 101), ("carol", 102)] {
            let mut event = e(actor, time);
            event.urls = vec!["https://example.test/campaign".into()];
            event.domains = vec!["example.test".into()];
            a.observe(event);
        }
        let detail = a.account_activity("alice");
        assert_eq!(detail.first_seen.unwrap().timestamp(), 100);
        assert_eq!(detail.windows.len(), 2);
        assert_eq!(detail.unique_domains, ["example.test"]);
        assert_eq!(detail.recent_events.len(), 1);
        assert_eq!(detail.cluster_ids.len(), 1);
        assert!(detail.scores.coordination_score > 0.0);
    }
    #[test]
    fn organic_burst_does_not_form_cluster() {
        let a = Analyzer::new(30, 3, 3, 24);
        for (i, target) in ["at://one", "at://two", "at://three", "at://four"]
            .iter()
            .enumerate()
        {
            let mut event = e(&format!("did:plc:organic{i}"), 100 + i as i64);
            event.target = Some((*target).into());
            assert!(a.observe(event).is_none());
        }
    }

    #[test]
    fn near_duplicate_simhash_forms_content_cluster() {
        let a = Analyzer::new(30, 3, 3, 24);
        for (i, simhash) in [0b1010u64, 0b1011, 0b1110].into_iter().enumerate() {
            let mut event = e(&format!("did:plc:spam{i}"), 100 + i as i64);
            event.target = None;
            event.normalized_text_hash = Some(100 + i as u64);
            event.simhash = Some(simhash);
            let detection = a.observe(event);
            if i < 2 {
                assert!(detection.is_none());
            } else {
                let detection = detection.expect("third near duplicate should form a cluster");
                assert_eq!(detection.accounts.len(), 3);
                assert_eq!(detection.scores.content_similarity_score, 1.0);
            }
        }
    }

    #[test]
    fn retention_uses_event_time_for_replay_and_expires_old_state() {
        let a = Analyzer::new(30, 2, 2, 1);
        a.observe(e("old", 100));
        assert_eq!(a.account("old").events_24h, 1);

        a.observe(e("new", 100 + 3601));
        assert_eq!(a.account("old").events_24h, 0);
        assert_eq!(a.account("new").events_24h, 1);
    }

    #[test]
    fn timing_evidence_is_the_median_consecutive_gap() {
        let events = vec![e("a", 0), e("b", 1), e("c", 11), e("d", 12)];
        assert_eq!(median_timing_difference_ms(&events), 1_000.0);
    }

    #[test]
    fn dissimilar_content_does_not_match_merely_because_it_has_fingerprints() {
        let a = Analyzer::new(30, 3, 3, 24);
        for (i, simhash) in [0u64, u64::MAX, 0xaaaa_aaaa_aaaa_aaaa]
            .into_iter()
            .enumerate()
        {
            let mut event = e(&format!("did:plc:organic{i}"), 100 + i as i64);
            event.target = None;
            event.normalized_text_hash = Some(i as u64 + 1);
            event.simhash = Some(simhash);
            assert!(a.observe(event).is_none());
        }
    }

    #[test]
    fn configured_sliding_windows_are_reported_and_independent() {
        let a = Analyzer::with_windows(30, 3, 3, 24, vec![60, 10, 60, 0]);
        let now = Utc::now();
        let mut recent = e("recent", now.timestamp());
        recent.timestamp = now;
        recent.time_us = now.timestamp_micros();
        a.observe(recent);
        let mut older = e("older", (now - Duration::seconds(30)).timestamp());
        older.timestamp = now - Duration::seconds(30);
        older.time_us = older.timestamp.timestamp_micros();
        a.observe(older);

        let rates = a.rates();
        assert_eq!(rates.windows.len(), 2);
        assert_eq!(rates.windows[0].seconds, 10);
        assert_eq!(rates.windows[1].seconds, 60);
        assert_eq!(rates.windows[0].events_per_sec, 0.1);
        assert_eq!(rates.windows[1].events_per_sec, 2.0 / 60.0);
        assert_eq!(rates.events_per_sec, rates.windows[1].events_per_sec);
    }

    #[test]
    fn round_the_clock_coverage_increases_automation_evidence() {
        let base = chrono::DateTime::from_timestamp(10_000, 0).unwrap();
        let round_the_clock = Analyzer::new(30, 100, 100, 25);
        let daytime = Analyzer::new(30, 100, 100, 25);
        for i in 0..24 {
            let mut all_hours = e("always-on", (base + Duration::hours(i)).timestamp());
            all_hours.timestamp = base + Duration::hours(i);
            round_the_clock.observe(all_hours);

            let mut office_hours = e("daytime", (base + Duration::minutes(i * 20)).timestamp());
            office_hours.timestamp = base + Duration::minutes(i * 20);
            daytime.observe(office_hours);
        }
        let broad = round_the_clock.account("always-on").automation_score;
        let narrow = daytime.account("daytime").automation_score;
        assert!(broad > narrow + 0.1, "broad={broad}, narrow={narrow}");
    }

    #[test]
    fn fingerprints_and_conversation_growth_use_event_time() {
        let a = Analyzer::new(30, 10, 10, 24);
        for (i, offset) in [-400, -350, -30, -20, -10].into_iter().enumerate() {
            let mut event = e(&format!("actor-{i}"), 10_000 + offset);
            event.kind = EventKind::Reply;
            event.reply_root = Some("at://conversation/root".into());
            event.normalized_text_hash = Some(42);
            event.text = Some("A repeated phrase".into());
            a.observe(event);
        }
        let fingerprint = &a.trending_fingerprints()[0];
        assert_eq!(fingerprint.count, 5);
        assert_eq!(fingerprint.sample.as_deref(), Some("A repeated phrase"));
        let conversation = &a.growing_conversations()[0];
        assert_eq!(conversation.recent_count, 3);
        assert_eq!(conversation.growth, 1);
    }

    #[test]
    fn cluster_detail_has_timeline_and_graph_edges() {
        let a = Analyzer::new(30, 3, 3, 24);
        let mut id = None;
        for (i, actor) in ["a", "b", "c"].into_iter().enumerate() {
            let mut event = e(actor, 100 + i as i64);
            event.target_did = Some("did:plc:target".into());
            id = a.observe(event).map(|d| d.cluster_id);
        }
        let detail = a.cluster_detail(&id.unwrap()).unwrap();
        assert_eq!(detail.timeline[0].event_count, 3);
        assert_eq!(detail.graph_edges.len(), 3);
        assert!(
            detail
                .graph_edges
                .iter()
                .all(|edge| edge.target == "did:plc:target")
        );
    }

    #[test]
    fn recent_events_are_bounded_ordered_and_filterable() {
        let a = Analyzer::new(30, 100, 100, 24);
        for (actor, time, kind, text, domain) in [
            (
                "did:plc:alice",
                100,
                EventKind::Post,
                "hello world",
                "example.test",
            ),
            ("did:plc:bob", 101, EventKind::Like, "", ""),
            (
                "did:plc:carol",
                102,
                EventKind::Post,
                "campaign",
                "news.test",
            ),
        ] {
            let mut event = e(actor, time);
            event.kind = kind;
            event.text = (!text.is_empty()).then(|| text.into());
            event.domains = (!domain.is_empty())
                .then(|| domain.into())
                .into_iter()
                .collect();
            a.observe(event);
        }

        let initial = a.recent_events(RecentEventFilter {
            limit: 2,
            ..Default::default()
        });
        assert_eq!(
            initial.events.iter().map(|e| e.time_us).collect::<Vec<_>>(),
            vec![101_000_000, 102_000_000]
        );
        assert!(initial.has_more);
        assert_eq!(initial.retained_event_count, 3);

        let filtered = a.recent_events(RecentEventFilter {
            after_time_us: Some(99_000_000),
            kind: Some("post".into()),
            query: Some("EXAMPLE".into()),
            limit: 5000,
        });
        assert_eq!(filtered.events.len(), 1);
        assert_eq!(filtered.events[0].actor_did, "did:plc:alice");
        assert_eq!(filtered.next_after_time_us, Some(102_000_000));
    }

    #[test]
    fn trend_drilldowns_match_exact_values_and_paginate_newest_first() {
        let a = Analyzer::new(30, 100, 100, 24);
        for (actor, time, domain, url, hash, kind) in [
            (
                "alice",
                100,
                "example.test",
                "https://example.test/a",
                42,
                EventKind::Post,
            ),
            (
                "bob",
                101,
                "elsewhere.test",
                "https://elsewhere.test",
                7,
                EventKind::Post,
            ),
            (
                "carol",
                102,
                "example.test",
                "https://example.test/a",
                42,
                EventKind::Reply,
            ),
            (
                "dave",
                103,
                "example.test",
                "https://example.test/b",
                42,
                EventKind::Post,
            ),
        ] {
            let mut event = e(actor, time);
            event.kind = kind;
            event.domains = vec![domain.into()];
            event.urls = vec![url.into()];
            event.normalized_text_hash = Some(hash);
            a.observe(event);
        }

        let page = a.trend_drilldown(TrendDimension::Domain, "EXAMPLE.TEST", None, 1, 1);
        assert_eq!(page.total, 3);
        assert_eq!(page.events[0].actor_did, "carol");
        assert_eq!(page.offset, 1);
        assert_eq!(page.limit, 1);

        let posts = a.trend_drilldown(
            TrendDimension::Fingerprint(42),
            "000000000000002a",
            Some("post"),
            0,
            20,
        );
        assert_eq!(posts.total, 2);
        assert_eq!(
            posts
                .events
                .iter()
                .map(|event| event.actor_did.as_str())
                .collect::<Vec<_>>(),
            ["dave", "alice"]
        );

        let exact_url =
            a.trend_drilldown(TrendDimension::Url, "https://example.test/a", None, 0, 20);
        assert_eq!(exact_url.total, 2);
    }
}
