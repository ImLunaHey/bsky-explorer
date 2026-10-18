CREATE TABLE IF NOT EXISTS follow_record_versions
(
 at_uri String, time_us Int64, target_did String, operation LowCardinality(String)
) ENGINE = ReplacingMergeTree
ORDER BY (at_uri, time_us, operation, target_did);

CREATE MATERIALIZED VIEW IF NOT EXISTS follow_record_versions_mv TO follow_record_versions AS
SELECT at_uri,time_us,toString(target_did) AS target_did,operation FROM events
WHERE collection='app.bsky.graph.follow' AND at_uri!='';

CREATE TABLE IF NOT EXISTS follow_deletions
(
 time_us Int64, event_id String, timestamp DateTime64(3,'UTC'), actor_did String, at_uri String
) ENGINE = ReplacingMergeTree ORDER BY (time_us,event_id);

CREATE MATERIALIZED VIEW IF NOT EXISTS follow_deletions_mv TO follow_deletions AS
SELECT time_us,event_id,timestamp,toString(actor_did) AS actor_did,at_uri FROM events
WHERE collection='app.bsky.graph.follow' AND operation='delete' AND target_did='' AND at_uri!='';

CREATE TABLE IF NOT EXISTS unfollow_resolutions
(
 event_id String, time_us Int64, actor_did String, target_did String, at_uri String,
 resolved_at DateTime64(3,'UTC')
) ENGINE = ReplacingMergeTree(resolved_at) ORDER BY event_id;
