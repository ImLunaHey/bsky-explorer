# Unfollow attribution

`resolve_follows` is an independent Rust worker. It uses only ClickHouse and does
not consume the firehose, call remote APIs, or modify ingestion cursors/raw events.
Migration 011 captures follow versions (including deletion tombstones) and unknown
deletes from new event inserts. A restartable newest-day-first seed indexes retained
history. State lives in ClickHouse snapshots; no local volume is required.

Each deletion is matched to the latest strictly earlier version of its record URI.
Missing, conflicting, future, and deleted subjects are left unknown. Resolved events
are appended to both accounts' history, then recorded in a completion ledger.
Readers deduplicate event IDs and prefer attributed targets, so crash/replay is safe.
Historical scanning repeats to pick up late-arriving versions. Limits: 500 deletes
per batch, two query threads, bounded memory/time, and two seconds between cycles.

Build the runtime image, deploy API (creates the views), then start one worker:

```
node deploy/update-service.mjs api IMAGE
node deploy/start-follow-resolution.mjs IMAGE
```

Neither step restarts the ingester. The start helper refuses an existing container
name; do not run multiple workers. Monitor `docker logs
bsky-explorer-follow-resolution-1`, its heartbeat health, and `/api/follow-resolution`.
Status reports global indexed deletes, attributed deletes and remaining seed days.
Seed completion is not attribution completion. Unmatched deletes are retried but
cannot be recovered without a retained prior follow. Raw event endpoints remain
unaltered; repaired counts are exposed through account history/follow analytics.

Attributed losses are always a partial lower bound, not a complete unfollow count.
Zero does not establish no losses; partial balance is not actual follower growth.
