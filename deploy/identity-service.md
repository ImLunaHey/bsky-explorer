# Identity service

`identity_service` is an independently deployable Rust HTTP service. It owns the
existing SQLite identity queue and a persistent DID/display-handle cache in that
database. Do not run two owners of this directory. API and ingester roles no
longer open the queue; `SERVICE_ROLE=all` retains the embedded worker for local
development only.

## Policy

- Known mappings do not expire or trigger upstream lookups on page views.
- Jetstream identity events with a handle populate/update mappings. An omitted
  handle does not erase a mapping; explicit invalid handles do.
- The ingester writes observations to ClickHouse independently. An insertion-time
  ordered materialized feed lets the service consume them every two seconds,
  including after downtime. Checkpoints and a one-minute replay overlap are
  durable. Older observations cannot replace newer mappings.
- Existing ClickHouse mappings are imported in resumable keyset pages. Cold reads
  use one ClickHouse query per batch, then populate the persistent and memory caches.
- Only absent mappings are queued. Known mappings cancel pending jobs, and worker
  completions are lease-guarded. Negative upstream results cool down for 24 hours;
  repeated visible requests do not bypass the cooldown.
- Visible accounts get priority. Every fifth worker batch prioritises background
  work. One worker batches up to 25 actors with a shared request budget (default
  two requests/second) and service-wide Retry-After backoff.
- Backlog discovery walks distinct accounts in `account_history_events`, including
  actors and targets, 250 at a time. It persists its cursor, wraps to pick up newly
  observed accounts, and pauses when 5,000 jobs are active. It does not claim to
  enumerate accounts we have never observed.
- Cached handles are display metadata, not authentication proofs. Identity events
  are best-effort; no periodic positive refresh means missed changes can stay stale.
- Remote misses first use the existing Bluesky AppView batch endpoint. Missing
  profiles fall back to `did:plc`/hostname-only `did:web` documents, with a reverse
  handle check via `com.atproto.identity.resolveHandle`. All requests share the
  budget. did:web DNS is checked and pinned to public addresses, with no redirects
  or environment proxies. Negative results are not proof that a DID is invalid.
- Successful remote lookups enter a durable SQLite outbox before completion.
  ClickHouse publication retries independently during outages.

## HTTP (private Docker network only)

- `POST /resolve`: JSON array of 1–100 DIDs; ordered records, immediate cached
  results, misses queued asynchronously. Body limit 32 KiB.
- `GET /api/identity-queue` and `/api/identity-queue/{id}`: existing admin views.
- `GET /stats`: displayed-account coverage counters, queue totals, bootstrap and
  observation/backlog checkpoints. Counters reset on restart; queue/cache persist.
- `GET /readyz`: HTTP readiness.

API uses `IDENTITY_SERVICE_URL=http://identity:3000`. If unavailable, display
lookups fall back after 1.5 seconds to a single ClickHouse batch; no remote lookup
is made in a page request. The service has no published host port.

## Existing deployment migration

Build a runtime image and test it first. This initial extraction needs **one**
ingester replacement to stop the old embedded worker and fix omitted-handle event
parsing. It reconnects using the existing archive cursor. API and resolver updates
after this migration do not restart collection.

```sh
node deploy/update-service.mjs ingester IMAGE --allow-ingester-restart
node deploy/start-identity-service.mjs IMAGE
IDENTITY_SERVICE_URL=http://identity:3000 node deploy/update-service.mjs api IMAGE
```

The start helper preserves the actual deployed archive path and existing queue;
it refuses to start until the ingester uses the same new image. It defaults the
upstream budget to two requests/second. Secrets stay in Docker configuration.
Stopped API/ingester rollback containers are retained. Before rolling back to an
old ingester with an embedded identity worker, **stop the identity service first**
to avoid two owners. SQLite schema additions are additive.

Future identity-only updates:

```sh
node deploy/update-service.mjs identity IMAGE
```

Never use an unrestricted Compose recreation for this migration. A fresh Compose
installation includes the `identity` service automatically. Existing installations
must preserve their actual `ARCHIVE_ROOT` when using Compose.
