CREATE TABLE IF NOT EXISTS observatory.identity_history
(
 did LowCardinality(String), handle LowCardinality(String),
 observed_at DateTime64(3, 'UTC'), source LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(observed_at)
ORDER BY (did, observed_at)
TTL observed_at + INTERVAL 365 DAY;

CREATE TABLE IF NOT EXISTS observatory.identities
(
 did LowCardinality(String), handle LowCardinality(String),
 observed_at DateTime64(3, 'UTC'), source LowCardinality(String)
)
ENGINE = ReplacingMergeTree(observed_at)
ORDER BY did;
