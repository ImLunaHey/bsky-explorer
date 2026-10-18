CREATE TABLE IF NOT EXISTS detection_accounts (
    did String,
    cluster_id String,
    last_seen DateTime64(3, 'UTC')
) ENGINE = ReplacingMergeTree(last_seen)
ORDER BY (did, cluster_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS detection_accounts_mv TO detection_accounts AS
SELECT arrayJoin(JSONExtract(evidence_json, 'accounts', 'Array(String)')) AS did,
       cluster_id, last_seen
FROM detections;

CREATE TABLE IF NOT EXISTS explorer_snapshots (
    key String,
    generated_at DateTime64(3, 'UTC'),
    payload String CODEC(ZSTD(3))
) ENGINE = ReplacingMergeTree(generated_at)
ORDER BY key;

ALTER TABLE events ADD INDEX IF NOT EXISTS actor_did_bloom actor_did TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE events ADD INDEX IF NOT EXISTS at_uri_bloom at_uri TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE events ADD INDEX IF NOT EXISTS reply_parent_bloom reply_parent TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE events ADD INDEX IF NOT EXISTS reply_root_bloom reply_root TYPE bloom_filter(0.01) GRANULARITY 4;
ALTER TABLE events ADD INDEX IF NOT EXISTS quote_target_bloom quote_target TYPE bloom_filter(0.01) GRANULARITY 4;
