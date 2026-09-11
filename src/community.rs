use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use xxhash_rust::xxh3::xxh3_64;

use crate::storage::Storage;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunityConfig {
    pub window_seconds: u64,
    pub event_types: BTreeSet<String>,
    pub event_weights: BTreeMap<String, f64>,
    pub min_edge_weight: f64,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_iterations: usize,
}

impl Default for CommunityConfig {
    fn default() -> Self {
        Self {
            window_seconds: 3_600,
            event_types: ["like", "repost", "follow", "reply", "quote"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            event_weights: BTreeMap::new(),
            min_edge_weight: 2.0,
            max_nodes: 50_000,
            max_edges: 250_000,
            max_iterations: 20,
        }
    }
}

impl CommunityConfig {
    fn normalized(mut self) -> Self {
        self.window_seconds = self.window_seconds.max(1);
        self.min_edge_weight = if self.min_edge_weight.is_finite() {
            self.min_edge_weight.max(0.0)
        } else {
            1.0
        };
        self.max_nodes = self.max_nodes.max(2);
        self.max_edges = self.max_edges.max(1);
        self.max_iterations = self.max_iterations.clamp(1, 100);
        self.event_weights
            .retain(|_, weight| weight.is_finite() && *weight > 0.0);
        self
    }
}

#[derive(Debug, Clone)]
pub struct CommunityInteraction {
    pub actor_did: String,
    pub target_did: String,
    pub kind: String,
    pub timestamp: DateTime<Utc>,
    /// Optional source-provided weight. Non-positive/non-finite values are ignored.
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunityMembership {
    pub community_id: String,
    pub did: String,
    pub weighted_degree: f64,
    pub internal_weight: f64,
    pub external_weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunitySummary {
    pub community_id: String,
    pub account_count: usize,
    pub internal_event_count: u64,
    pub internal_weight: f64,
    pub total_weight: f64,
    pub top_members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunityEdge {
    pub source_community: String,
    pub target_community: String,
    pub weight: f64,
    pub event_count: u64,
    pub event_types: Vec<String>,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunityMemberEdge {
    pub community_id: String,
    pub source_did: String,
    pub target_did: String,
    pub weight: f64,
    pub event_count: u64,
    pub event_types: Vec<String>,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommunitySnapshot {
    pub snapshot_id: String,
    pub generated_at: DateTime<Utc>,
    pub window_seconds: u64,
    pub node_count: usize,
    pub edge_count: usize,
    pub truncated_nodes: usize,
    pub truncated_edges: usize,
    pub parameters: CommunityConfig,
    pub communities: Vec<CommunitySummary>,
    pub memberships: Vec<CommunityMembership>,
    pub member_edges: Vec<CommunityMemberEdge>,
    pub cross_community_edges: Vec<CommunityEdge>,
}

#[derive(Clone, Default)]
struct WeightedEdge {
    weight: f64,
    events: u64,
    event_types: BTreeSet<String>,
    last_seen: Option<DateTime<Utc>>,
}

/// Builds an undirected account graph and applies deterministic weighted label
/// propagation. Input ordering cannot affect the result. Self-interactions,
/// invalid weights, disallowed event types, and events outside the window are ignored.
pub fn detect_communities(
    interactions: impl IntoIterator<Item = CommunityInteraction>,
    generated_at: DateTime<Utc>,
    config: CommunityConfig,
) -> CommunitySnapshot {
    let config = config.normalized();
    let earliest =
        generated_at - Duration::seconds(config.window_seconds.min(i64::MAX as u64) as i64);
    let mut candidates: BTreeMap<(String, String), WeightedEdge> = BTreeMap::new();
    for interaction in interactions {
        if interaction.timestamp < earliest
            || interaction.timestamp > generated_at
            || interaction.actor_did == interaction.target_did
            || interaction.actor_did.is_empty()
            || interaction.target_did.is_empty()
            || !config.event_types.contains(&interaction.kind)
            || !interaction.weight.is_finite()
            || interaction.weight <= 0.0
        {
            continue;
        }
        let multiplier = config
            .event_weights
            .get(&interaction.kind)
            .copied()
            .unwrap_or(1.0);
        let pair = if interaction.actor_did < interaction.target_did {
            (interaction.actor_did, interaction.target_did)
        } else {
            (interaction.target_did, interaction.actor_did)
        };
        let edge = candidates.entry(pair).or_default();
        edge.weight += interaction.weight * multiplier;
        edge.events = edge.events.saturating_add(1);
        edge.event_types.insert(interaction.kind);
        edge.last_seen = Some(edge.last_seen.map_or(interaction.timestamp, |seen| {
            seen.max(interaction.timestamp)
        }));
    }

    let mut edges: Vec<_> = candidates
        .into_iter()
        .filter(|(_, edge)| edge.weight >= config.min_edge_weight)
        .collect();
    let candidate_edge_count = edges.len();
    let mut node_strength: HashMap<String, f64> = HashMap::new();
    for ((left, right), edge) in &edges {
        *node_strength.entry(left.clone()).or_default() += edge.weight;
        *node_strength.entry(right.clone()).or_default() += edge.weight;
    }
    let candidate_node_count = node_strength.len();
    let mut ranked_nodes: Vec<_> = node_strength.into_iter().collect();
    ranked_nodes.sort_by(|(left_did, left_weight), (right_did, right_weight)| {
        right_weight
            .total_cmp(left_weight)
            .then(left_did.cmp(right_did))
    });
    ranked_nodes.truncate(config.max_nodes);
    let retained: BTreeSet<_> = ranked_nodes.into_iter().map(|(did, _)| did).collect();
    edges.retain(|((left, right), _)| retained.contains(left) && retained.contains(right));
    edges.sort_by(|(left_pair, left), (right_pair, right)| {
        right
            .weight
            .total_cmp(&left.weight)
            .then(right.events.cmp(&left.events))
            .then(left_pair.cmp(right_pair))
    });
    edges.truncate(config.max_edges);
    edges.sort_by(|(left, _), (right, _)| left.cmp(right));

    let nodes: Vec<_> = edges
        .iter()
        .flat_map(|((left, right), _)| [left.clone(), right.clone()])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let indexes: HashMap<_, _> = nodes
        .iter()
        .enumerate()
        .map(|(index, did)| (did.as_str(), index))
        .collect();
    let mut adjacency = vec![Vec::<(usize, f64)>::new(); nodes.len()];
    for ((left, right), edge) in &edges {
        let left = indexes[left.as_str()];
        let right = indexes[right.as_str()];
        adjacency[left].push((right, edge.weight));
        adjacency[right].push((left, edge.weight));
    }
    let mut labels: Vec<usize> = (0..nodes.len()).collect();
    for _ in 0..config.max_iterations {
        let mut changed = false;
        for node in 0..nodes.len() {
            let mut scores = BTreeMap::<usize, f64>::new();
            for &(neighbor, weight) in &adjacency[node] {
                *scores.entry(labels[neighbor]).or_default() += weight;
            }
            if let Some((&best, _)) =
                scores
                    .iter()
                    .max_by(|(left_label, left), (right_label, right)| {
                        left.total_cmp(right)
                            .then_with(|| right_label.cmp(left_label))
                    })
                && best != labels[node]
            {
                labels[node] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for (node, label) in labels.iter().copied().enumerate() {
        groups.entry(label).or_default().push(node);
    }
    let community_ids: HashMap<usize, String> = groups
        .iter()
        .map(|(&label, members)| {
            let canonical = members
                .iter()
                .map(|&node| nodes[node].as_str())
                .min()
                .unwrap_or("");
            (label, format!("com-{:016x}", xxh3_64(canonical.as_bytes())))
        })
        .collect();
    let mut degrees = vec![0.0; nodes.len()];
    let mut internal = vec![0.0; nodes.len()];
    let mut summary_values = BTreeMap::<usize, (u64, f64, f64)>::new();
    let mut cross = BTreeMap::<(String, String), WeightedEdge>::new();
    for ((left, right), edge) in &edges {
        let left = indexes[left.as_str()];
        let right = indexes[right.as_str()];
        degrees[left] += edge.weight;
        degrees[right] += edge.weight;
        if labels[left] == labels[right] {
            internal[left] += edge.weight;
            internal[right] += edge.weight;
            let values = summary_values.entry(labels[left]).or_default();
            values.0 += edge.events;
            values.1 += edge.weight;
        } else {
            let left_id = &community_ids[&labels[left]];
            let right_id = &community_ids[&labels[right]];
            let pair = if left_id < right_id {
                (left_id.clone(), right_id.clone())
            } else {
                (right_id.clone(), left_id.clone())
            };
            let value = cross.entry(pair).or_default();
            value.weight += edge.weight;
            value.events += edge.events;
            value.event_types.extend(edge.event_types.iter().cloned());
            value.last_seen = value.last_seen.max(edge.last_seen);
        }
        summary_values.entry(labels[left]).or_default().2 += edge.weight;
        summary_values.entry(labels[right]).or_default().2 += edge.weight;
    }
    let mut memberships = Vec::with_capacity(nodes.len());
    for (&label, members) in &groups {
        let community_id = community_ids[&label].clone();
        for &node in members {
            memberships.push(CommunityMembership {
                community_id: community_id.clone(),
                did: nodes[node].clone(),
                weighted_degree: degrees[node],
                internal_weight: internal[node],
                external_weight: degrees[node] - internal[node],
            });
        }
    }
    let communities = groups
        .iter()
        .map(|(&label, members)| {
            let values = summary_values.get(&label).copied().unwrap_or_default();
            let mut top = members.to_vec();
            top.sort_by(|&left, &right| {
                degrees[right]
                    .total_cmp(&degrees[left])
                    .then(nodes[left].cmp(&nodes[right]))
            });
            CommunitySummary {
                community_id: community_ids[&label].clone(),
                account_count: members.len(),
                internal_event_count: values.0,
                internal_weight: values.1,
                total_weight: values.2,
                top_members: top
                    .into_iter()
                    .take(10)
                    .map(|node| nodes[node].clone())
                    .collect(),
            }
        })
        .collect();
    let member_edges = edges
        .iter()
        .filter_map(|((left, right), edge)| {
            let left_node = indexes[left.as_str()];
            let right_node = indexes[right.as_str()];
            (labels[left_node] == labels[right_node]).then(|| CommunityMemberEdge {
                community_id: community_ids[&labels[left_node]].clone(),
                source_did: left.clone(),
                target_did: right.clone(),
                weight: edge.weight,
                event_count: edge.events,
                event_types: edge.event_types.iter().cloned().collect(),
                last_seen: edge.last_seen.unwrap_or(generated_at),
            })
        })
        .collect();
    let cross_community_edges = cross
        .into_iter()
        .map(
            |((source_community, target_community), edge)| CommunityEdge {
                source_community,
                target_community,
                weight: edge.weight,
                event_count: edge.events,
                event_types: edge.event_types.into_iter().collect(),
                last_seen: edge.last_seen.unwrap_or(generated_at),
            },
        )
        .collect();
    CommunitySnapshot {
        snapshot_id: format!(
            "community-{}-{}",
            config.window_seconds,
            generated_at.timestamp_millis()
        ),
        generated_at,
        window_seconds: config.window_seconds,
        node_count: nodes.len(),
        edge_count: edges.len(),
        truncated_nodes: candidate_node_count.saturating_sub(nodes.len()),
        truncated_edges: candidate_edge_count.saturating_sub(edges.len()),
        parameters: config,
        communities,
        memberships,
        member_edges,
        cross_community_edges,
    }
}

/// Loads a bounded retained interaction slice, builds a snapshot off the HTTP
/// request path, and publishes it only after all snapshot rows are durable.
pub async fn build_and_store_snapshot(
    storage: &Storage,
    config: CommunityConfig,
    max_input_rows: u64,
) -> anyhow::Result<CommunitySnapshot> {
    let generated_at = Utc::now();
    let event_types: Vec<_> = config.event_types.iter().cloned().collect();
    let interactions = storage
        .community_interactions(config.window_seconds, &event_types, max_input_rows)
        .await?;
    let snapshot =
        tokio::task::spawn_blocking(move || detect_communities(interactions, generated_at, config))
            .await?;
    storage.insert_community_snapshot(&snapshot).await?;
    Ok(snapshot)
}

pub async fn run_periodically(
    storage: Storage,
    configs: Vec<CommunityConfig>,
    max_input_rows: u64,
    interval: std::time::Duration,
) {
    let mut ticker = tokio::time::interval(interval.max(std::time::Duration::from_secs(60)));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        for config in &configs {
            match build_and_store_snapshot(&storage, config.clone(), max_input_rows).await {
                Ok(snapshot) => tracing::info!(
                    snapshot_id = %snapshot.snapshot_id,
                    window_seconds = snapshot.window_seconds,
                    communities = snapshot.communities.len(),
                    nodes = snapshot.node_count,
                    edges = snapshot.edge_count,
                    "community snapshot completed"
                ),
                Err(error) => tracing::warn!(
                    window_seconds = config.window_seconds,
                    %error,
                    "community snapshot failed"
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interaction(a: &str, b: &str, weight: f64) -> CommunityInteraction {
        CommunityInteraction {
            actor_did: a.into(),
            target_did: b.into(),
            kind: "repost".into(),
            timestamp: DateTime::from_timestamp(1_000, 0).unwrap(),
            weight,
        }
    }

    #[test]
    fn separates_dense_organic_groups_and_exposes_bridge() {
        let now = DateTime::from_timestamp(1_001, 0).unwrap();
        let mut events = Vec::new();
        for (a, b) in [
            ("a", "b"),
            ("a", "c"),
            ("b", "c"),
            ("x", "y"),
            ("x", "z"),
            ("y", "z"),
        ] {
            events.push(interaction(a, b, 5.0));
        }
        events.push(interaction("c", "x", 1.0));
        let snapshot = detect_communities(
            events,
            now,
            CommunityConfig {
                min_edge_weight: 1.0,
                ..Default::default()
            },
        );
        assert_eq!(snapshot.communities.len(), 2);
        assert_eq!(snapshot.cross_community_edges.len(), 1);
        assert_eq!(snapshot.cross_community_edges[0].weight, 1.0);
    }

    #[test]
    fn result_is_deterministic_across_input_order() {
        let now = DateTime::from_timestamp(1_001, 0).unwrap();
        let events = vec![
            interaction("c", "a", 3.0),
            interaction("b", "a", 2.0),
            interaction("c", "b", 4.0),
        ];
        let mut reversed = events.clone();
        reversed.reverse();
        assert_eq!(
            detect_communities(events, now, Default::default()),
            detect_communities(reversed, now, Default::default())
        );
    }

    #[test]
    fn graph_respects_node_and_edge_bounds() {
        let now = DateTime::from_timestamp(1_001, 0).unwrap();
        let events = (0..100)
            .map(|index| interaction(&format!("did:{index}"), &format!("did:{}", index + 1), 10.0));
        let snapshot = detect_communities(
            events,
            now,
            CommunityConfig {
                min_edge_weight: 1.0,
                max_nodes: 12,
                max_edges: 5,
                ..Default::default()
            },
        );
        assert!(snapshot.node_count <= 12);
        assert!(snapshot.edge_count <= 5);
        assert!(snapshot.truncated_nodes > 0);
        assert!(snapshot.truncated_edges > 0);
    }

    #[test]
    fn filters_window_type_self_edges_and_minimum_weight() {
        let now = DateTime::from_timestamp(2_000, 0).unwrap();
        let mut valid = interaction("a", "b", 2.0);
        valid.timestamp = now;
        let mut old = interaction("b", "c", 10.0);
        old.timestamp = now - Duration::hours(2);
        let mut wrong_type = interaction("c", "d", 10.0);
        wrong_type.kind = "post".into();
        let snapshot = detect_communities(
            vec![valid, old, wrong_type, interaction("e", "e", 10.0)],
            now,
            Default::default(),
        );
        assert_eq!(snapshot.node_count, 2);
        assert_eq!(snapshot.edge_count, 1);
    }
}
