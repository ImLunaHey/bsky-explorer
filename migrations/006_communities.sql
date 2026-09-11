CREATE TABLE IF NOT EXISTS observatory.community_snapshots
(
    snapshot_id String CODEC(ZSTD(3)),
    status LowCardinality(String),
    window_seconds UInt64,
    generated_at DateTime64(3, 'UTC'),
    completed_at Nullable(DateTime64(3, 'UTC')),
    community_count UInt64,
    node_count UInt64,
    edge_count UInt64,
    truncated_nodes UInt64,
    truncated_edges UInt64,
    parameters_json String CODEC(ZSTD(3)),
    error String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree(generated_at)
ORDER BY (window_seconds, snapshot_id);

CREATE TABLE IF NOT EXISTS observatory.community_summaries
(
    snapshot_id String CODEC(ZSTD(3)),
    community_id String CODEC(ZSTD(3)),
    account_count UInt64,
    internal_event_count UInt64,
    internal_weight Float64,
    total_weight Float64,
    density Float64,
    score Float64,
    top_members Array(String) CODEC(ZSTD(3))
) ENGINE = MergeTree
ORDER BY (snapshot_id, score, community_id);

CREATE TABLE IF NOT EXISTS observatory.community_memberships
(
    snapshot_id String CODEC(ZSTD(3)),
    community_id String CODEC(ZSTD(3)),
    did String CODEC(ZSTD(3)),
    weighted_degree Float64,
    internal_weight Float64,
    external_weight Float64
) ENGINE = MergeTree
ORDER BY (snapshot_id, community_id, did);

CREATE TABLE IF NOT EXISTS observatory.community_edges
(
    snapshot_id String CODEC(ZSTD(3)),
    source_community_id String CODEC(ZSTD(3)),
    target_community_id String CODEC(ZSTD(3)),
    weight Float64,
    event_count UInt64,
    types Array(String) CODEC(ZSTD(3)),
    last_seen DateTime64(3, 'UTC')
) ENGINE = MergeTree
ORDER BY (snapshot_id, source_community_id, target_community_id);

CREATE TABLE IF NOT EXISTS observatory.community_member_edges
(
    snapshot_id String CODEC(ZSTD(3)),
    community_id String CODEC(ZSTD(3)),
    source_did String CODEC(ZSTD(3)),
    target_did String CODEC(ZSTD(3)),
    weight Float64,
    event_count UInt64,
    types Array(String) CODEC(ZSTD(3)),
    last_seen DateTime64(3, 'UTC')
) ENGINE = MergeTree
ORDER BY (snapshot_id, community_id, weight, source_did, target_did);
