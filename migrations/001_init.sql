CREATE DATABASE IF NOT EXISTS observatory;

CREATE TABLE IF NOT EXISTS observatory.events
(
 event_id String, time_us Int64, timestamp DateTime64(3, 'UTC'), actor_did LowCardinality(String),
 kind LowCardinality(String), collection LowCardinality(String), rkey String,
 operation LowCardinality(String), at_uri String, record_json String CODEC(ZSTD(3)),
 target String, target_did LowCardinality(String), text String CODEC(ZSTD(3)),
 reply_parent String, reply_root String, quote_target String,
 urls Array(String), domains Array(LowCardinality(String)), languages Array(LowCardinality(String)),
 text_hash UInt64, normalized_text_hash UInt64, simhash UInt64,
 INDEX target_bloom target TYPE bloom_filter(0.01) GRANULARITY 4,
 INDEX text_hash_set normalized_text_hash TYPE set(10000) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (toDate(timestamp), kind, actor_did, timestamp, event_id)
TTL timestamp + INTERVAL 365 DAY
SETTINGS index_granularity = 8192;

CREATE TABLE IF NOT EXISTS observatory.detections
(
 cluster_id String, first_seen DateTime64(3, 'UTC'), last_seen DateTime64(3, 'UTC'),
 account_count UInt32, event_count UInt64, coordination_score Float64, evidence_json String CODEC(ZSTD(3))
)
ENGINE = ReplacingMergeTree(last_seen)
PARTITION BY toYYYYMM(first_seen)
ORDER BY cluster_id;
