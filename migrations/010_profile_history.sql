CREATE TABLE IF NOT EXISTS account_history_events
(
 account_did String,
 timestamp DateTime64(3, 'UTC'),
 event_id String,
 actor_did String,
 target_did String,
 kind LowCardinality(String),
 operation LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (account_did, timestamp, event_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS account_history_events_mv
TO account_history_events AS
SELECT arrayJoin(arrayDistinct(arrayFilter(did -> did != '', [toString(actor_did),toString(target_did)]))) AS account_did,
 timestamp,event_id,toString(actor_did) AS actor_did,toString(target_did) AS target_did,kind,operation
FROM events WHERE operation IN ('create','delete');
