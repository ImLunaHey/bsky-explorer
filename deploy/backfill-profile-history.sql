-- Run AFTER migration 010. The materialized view captures concurrent live
-- inserts. Overlap is safe: history queries count distinct event IDs per bucket.
-- Re-running is logically safe but consumes additional storage; do not schedule.
INSERT INTO account_history_events
SELECT arrayJoin(arrayDistinct(arrayFilter(did -> did != '', [toString(actor_did),toString(target_did)]))) AS account_did,
 timestamp,event_id,toString(actor_did),toString(target_did),kind,operation
FROM events
WHERE operation IN ('create','delete') AND timestamp >= now()-INTERVAL 31 DAY
SETTINGS max_threads=2,max_memory_usage=1000000000,max_block_size=16384,max_insert_threads=1;
