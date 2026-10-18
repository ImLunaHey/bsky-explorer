# ATProto Explorer

A single-machine Rust + ClickHouse observatory for public ATProto activity. It connects to Jetstream from startup, archives the original JSON with Zstandard compression, normalizes records from every public ATProto collection, maintains rolling hot state, and emits evidence-led coordination scores. Known Bluesky lexicons receive dedicated fields; arbitrary lexicons retain their repository operation and compact sanitized record JSON for future search/reprocessing. Blob objects and inline media data are removed from normalized rows, and the application never downloads media/profile blobs or calls an LLM.

## Run on the storage array

Compose bind-mounts every durable byte under `/mnt/storage/bsky-observatory`. On this host the array has about 3.5 TB available, but the current account cannot create its application directory. Run once as an administrator:

```bash
sudo install -d -o "$USER" -g "$(id -gn)" /mnt/storage/bsky-observatory/{archive,clickhouse}
cp .env.example .env
docker compose up --build -d
```

Open <http://localhost:3000>. Prometheus metrics are at `/metrics`; JSON endpoints are under `/api`. Set `STORAGE_ROOT` for another array mount. Do not point it at this repository. Compose passes all retention, batching, window, and ClickHouse credential settings from `.env` into the service; the storage array remains the only durable-data location.

## ATProto exploration

### Enriched collections and music charts

`/collections` groups observed collections by category and renders schema-aware
cards for Teal/Rocksky music, Standard documents, Currents saves, Streamplace,
calendar events, Popfeed films, Podping, sensor batches, weather, flights and
Puzzmo streaks. Unknown schemas keep a text/field preview and raw record inspector.
Recent collection browsing is limited to 30 records observed in the last 24 hours.
External links are HTTP(S)-only; record text is never rendered as HTML. Cards do
not fetch remote artwork, resolve arbitrary schema URLs, or infer measurement units.

`/music` and `GET /api/music?window=24h` (`1h`, `24h`, `7d`) expose top-30 songs,
artists and albums, exact observed play/listener counts and listening activity.
Sources are `fm.teal.feed.play`, legacy `fm.teal.alpha.feed.play`, and
`app.rocksky.scrobble`. Catalog songs and now-playing statuses are not listens.
Adapters follow observed records and the [Teal play schema](https://tangled.org/byjp.me/rocksky/commit/cde26b06e7cc0040975fc4998b27b2ffbf76f242)
and [Rocksky lexicons](https://next.tangled.org/rocksky.app/rocksky/blob/main/LEXICONS.md).

The independent `enrichment` service scans only these collections over seven
days of retained event timestamps, selects the latest version per URI, excludes
observed deletions, then filters by declared playback time (`playedTime` or
`createdAt`). Undated, malformed, future and historical out-of-window plays are
excluded. Cross-app duplicates match account, case/whitespace-normalized song
and artist credits, and exact playback second. Different spellings, credits or
timestamps may remain separate; this is not canonical MusicBrainz resolution.
Album rankings use track artist credits, so compilations may split. Each credited
artist receives a play. Repeated listens count: a high rank may come from one
account, which is why listener counts are shown alongside plays.

Charts are retained snapshots, not complete or independently verified network
statistics. A serial worker refreshes every five minutes (one ClickHouse thread,
90-second query budget, 750 MB query memory, 500,000 record ceiling). On failure
or budget exhaustion it keeps the previous result instead of publishing a partial
ranking. The UI warns after 30 minutes without a completed refresh. HTTP requests
read only persisted snapshots; they never trigger archive aggregation.

For an existing split deployment, build the runtime image and start **only** the
new service (set the image tag and project name for your installation):

```bash
ENRICHMENT_IMAGE=bsky-explorer-api:enriched docker compose -p bsky-explorer up -d --no-deps enrichment
```

The worker has no archive mount and does not touch ingestion cursors or queues.
Do not recreate the existing ClickHouse or ingester to deploy enrichment. For a
one-shot refresh, use `ENRICHMENT_ONCE=1 cargo run --bin enrich_collections` with
the database environment configured. Test adapters with `cd frontend && npm test`
(Node 22.18+); Rust tests include an opt-in isolated ClickHouse integration test.

### Activity overview

The homepage explores observed public ATProto activity across the last hour, 24 hours,
or seven days. A shared time range controls active accounts, published posts, replies,
activity history, rising posts and conversations, languages, and shared domains.
Every metric comes from retained ClickHouse events, independently of the bounded
in-memory firehose buffer. Operations and investigations are grouped separately;
the former network dashboard remains available at `/system/network`.

`GET /api/overview?window=1h` (also `24h` and `7d`) returns one consistent,
half-open event-time window and an equal-length preceding comparison window.
Counts use `uniqCombined64` distinct-count estimates to bound memory and deduplicate
replayed event IDs. Posts include newly created posts, replies, and quotes; active
accounts include actors taking actions in any observed collection. Social posts,
reply counts and interaction graphs are explicitly Bluesky-specific. These are not
platform membership counts. Backfilled records use historical timestamps and can
change past results. Multilingual posts count in each declared language.

Rankings compare a bounded candidate set: the approximate 1,000 most active targets
in each comparison period, then rank positive interaction growth among that union.
Conversation rankings require at least two replying accounts. This is not an
exhaustive ranking of every thread. Post previews come from the comparison windows;
older roots remain clickable even when their text is unavailable in the overview.
Record drilldowns show available history, not time-filtered dashboard totals.

Ranking distinct counts use precision-14 sketches (roughly 0.8% typical relative
error), keeping thousands of candidate aggregation states within the worker memory
budget. They are estimates, not exact engagement counts.

Queries have memory, thread, and execution limits. A serial background worker
refreshes persisted snapshots (target intervals: 60/300/900 seconds; actual refresh
can be later under load). HTTP requests read only small saved responses. Last-good
snapshots survive restarts and failed refreshes; displayed window end is their
measurement time, not the current time. Dataset coverage refreshes every 30 minutes.
Cold installations return an explicit preparing response until snapshots exist.
These are precomputed responses, not incremental aggregation tables. Empty chart buckets mean no retained
activity, not verified network inactivity. Coverage and ingestion status remain
visible, and the connection indicator reads actual Jetstream state.

`/collections` browses the retained collection inventory and recent records, including
non-Bluesky schemas. Generic `/records/<encoded-at-uri>` pages show sanitized retained
JSON. Account pages open on their repository inventory; Bluesky profiles and posts are
optional views. Text/domain/collection search covers the last 24 hours; exact IDs
search retained history. No claim of complete network coverage is made.

## Independent deployments

Production Compose runs four application services:

- `ingester`: Jetstream, archive/checkpoint ownership, backfills, identity workers,
  analytics materializations and private hot-state endpoints. No published host port.
- `api`: durable queries and queue controls; explicitly proxies live state/SSE to
  `INGESTER_URL`. It never starts collection or background workers.
- `frontend`: static assets and the public reverse proxy on port 3000.
- `enrichment`: independent background music-chart materialization, with no archive mount.

The two Rust roles share ClickHouse and the archive's SQLite queues (WAL). Only the
ingester writes stream segments/checkpoints. SQLite queue controls still need write
access from the API. Keep these services on the same host/local filesystem.

For subsequent web-only deployments:

```bash
docker compose build api frontend
docker compose up -d --no-deps api frontend
```

Do **not** use an unrestricted `compose up --build` for web-only updates. That can
recreate ingestion. Use `SERVICE_ROLE=all` for legacy local development; `api`
requires an explicit private `INGESTER_URL`. An unavailable collector returns 502
for live endpoints while retained-data pages remain usable. SSE forwards reconnect
cursors and disables proxy buffering.

On an already-running installation, `node deploy/update-service.mjs api IMAGE`
or `node deploy/update-service.mjs frontend IMAGE` preserves the live environment,
mounts and networking in memory, waits for health, and retains a stopped rollback.
It never recreates a sibling service. Updating `ingester` is deliberately blocked
unless `--allow-ingester-restart` is explicitly supplied. Build images first with
`docker build --target runtime -t IMAGE .` or `--target frontend` respectively.

For this host, preserve `ARCHIVE_ROOT=/mnt/storage/bsky-observatory/archive-deploy-a8a891f`.
The historical ClickHouse container pins a different underlying mount from the
current host path; do not recreate it until its live data is migrated and verified.
Never restart the old stopped monolith alongside the ingester.

Migration 009 creates a DID-keyed investigation mapping and snapshot storage.
Existing detection history needs a one-time, bounded streaming backfill into
`detection_accounts`; the materialized view covers new inserts. Re-running the
backfill is safe because reads group by cluster and the table replaces duplicates.
Prewarm summaries using `refresh_explorer` with the same ClickHouse environment
before switching public traffic. No archive mount is needed for that command.

Validate the overview against an isolated ClickHouse server with:

```bash
CLICKHOUSE_TEST_URL=http://127.0.0.1:18123 cargo test --lib storage::overview -- --include-ignored
```

The integration test creates and removes its own `overview_test_<pid>` database.
Never point this test at an untrusted/shared test namespace.

## Source-only publishing

Ingested archives, databases, local environment files, keys, and build outputs must
remain outside Git. `fixtures/synthetic.jsonl` contains fabricated test scenarios,
not collected network data. Before publishing, scan the complete outgoing history:

```bash
gitleaks git . --redact --log-opts=HEAD
git ls-files
```

`.gitleaksignore` contains one reviewed historical false positive: a SQL upsert
field assignment, not an API credential. Do not add real secrets to that allowlist.

The observatory allocation defaults to 25 TiB. Change `STORAGE_BUDGET_TIB`, or set `STORAGE_BUDGET_BYTES` for an exact override. This logical budget is reported separately from filesystem capacity; runway projections show both and identify whichever limit arrives first at the currently measured daily growth rate.

Before replacing a running container, verify that its `/data/archive` mount and
the intended host archive directory refer to the same filesystem and current
cursor. A storage array mounted after Docker starts can hide an older root-disk
directory that an existing container is still using. Do not restart against a
stale copy. `ARCHIVE_ROOT` can pin the archive independently of ClickHouse's
`STORAGE_ROOT`. Preserve the live archive and queue databases, pause writers for
the final synchronization, and verify the copied cursor before switching.

The dashboard is a Vite/React application built into the production container. Axum serves its hashed assets with immutable caching, keeps the SPA entry point fresh, and marks live API responses `no-store`. During frontend development, run `npm install && npm run dev` in `frontend`; Vite proxies API calls to the Rust service on port 3000.

## Data and restart behavior

Jetstream JSON is flushed into rotating `.jsonl.zst` files before the microsecond cursor is atomically advanced. Reconnects request that cursor; a duplicate boundary is possible and preferable to upstream loss. The deterministic `event_id` supports future deduplication. Archives are the recovery/reprocessing source if ClickHouse is unavailable, while the bounded ingest channel applies backpressure.

Archive segments can be validated and reprocessed with the replay binary. It defaults to a read-only validation pass; add `--write` to batch normalized rows into ClickHouse. Replay snapshots segments in filename order, fails with file/line context on corrupt input, and never reads or advances the live Jetstream cursor. Stop ingestion or replay only completed/copied segments if the newest archive file is still being written. Replaying overlapping ranges may create duplicate physical rows, so use time bounds deliberately.

```bash
cargo run --bin replay -- --archive-dir /mnt/storage/bsky-observatory/archive
cargo run --bin replay -- --archive-dir /mnt/storage/bsky-observatory/archive \
  --since-time-us 1789060000000000 --until-time-us 1789063600000000 --write
```

ClickHouse uses monthly `MergeTree` partitions, time/kind/actor ordering, Zstd text/evidence/record compression, arrays for URLs/domains, and skip indexes. Raw records and normalized rows default to 30 and 365 day retention respectively. Generic record JSON is compact, valid JSON bounded to 64 KiB per event; nested ATProto blob values, legacy blob values, data URIs, and inline byte fields are stripped before serialization. Deletes remain queryable by DID, collection, rkey, AT URI, operation, and observation timestamp without inventing record content.

## API

- `GET /api/stats` — event/post/like/follow/interaction rates
- `GET /api/trending/urls` and `/api/trending/domains`
- `GET /api/anomalies`, `/api/clusters`, `/api/clusters/{id}`
- `GET /api/accounts/{did}` — rolling activity and automation evidence
- `GET /api/identities?dids=did:plc:one,did:plc:two` — batch DID-to-handle lookup (up to 100)
- `GET /api/storage` — archive, ClickHouse/derived sizes and observed growth projections
- `GET /api/system` — process/cgroup memory, load, uptime, array capacity, and per-table ClickHouse compression/row/part health
- `GET /api/system/history` — bounded 15-second samples for 24-hour dashboard charts (process-local; resets on restart)
- `GET /metrics` — Prometheus exposition
- `GET /healthz` — process liveness
- `GET /readyz` — readiness, including a ClickHouse query

Scores describe observed evidence (`automation`, coordination, shared-target, timing, and content similarity); they do not label accounts as bots. Cluster evidence includes participants, timing, shared targets, URLs/domains, and sample event IDs.

Jetstream identity events update a bounded in-memory DID/handle cache immediately and are persisted to both current and history tables in ClickHouse. Cache misses first consult ClickHouse, then use the configurable public Bluesky profile resolver with positive/negative TTLs and rate limiting. Handle changes therefore flow from the firehose without polling; successful on-demand resolutions are also retained. No profile media is requested or stored.

## Configuration and verification

The detailed in-memory analysis queue is independently bounded by
`HOT_DETAIL_RETENTION_SECONDS` (default one hour) and `HOT_DETAIL_MAX_EVENTS`
(default 250,000); the smaller bound wins. Compact per-second counters preserve
configured rate windows through 24 hours. Full normalized events remain in
ClickHouse and raw payloads remain in the compressed archive. Prometheus exposes
the current queue size as `observatory_hot_detail_events`.

Weighted account-community snapshots are built from retained interactions outside
the request path. `COMMUNITY_WINDOWS_SECONDS`, `COMMUNITY_EVENT_TYPES`, and
`COMMUNITY_MIN_EDGE_WEIGHT` select the graph inputs. Input rows, nodes, and edges
have independent hard bounds; `COMMUNITY_INTERVAL_SECONDS` controls refreshes.
The deterministic label-propagation result, memberships, internal graph edges,
and cross-community bridges are persisted in ClickHouse for historical comparison.

Identity enrichment is asynchronous. Durable jobs live on the storage array at
`DATA_DIR/identity-queue` (or `IDENTITY_QUEUE_DIR`), use leased restart recovery,
and are inspectable at `GET /api/identity-queue?limit=25&offset=0`. Queue state is
stored in an embedded SQLite database using WAL mode, so claims, summaries, and
pagination are indexed and retained history is not loaded into application memory.
On first startup, legacy per-job JSON files are imported transactionally and left
in `legacy-json-migration-backup/` as a recovery copy; later startups are
idempotent. Backfill jobs and repository/association cursor checkpoints use the
same SQLite WAL control plane, including leases, dependencies, counters, retries,
and errors. Active queue code never writes per-job JSON files.
`IDENTITY_WORKER_CONCURRENCY` defaults to 1, `IDENTITY_REQUESTS_PER_SECOND`
controls fixed-AppView pacing, and `IDENTITY_NEGATIVE_TTL_SECONDS` controls when
a missing profile may be retried. Identity API misses return immediately and
queue work rather than putting public-network latency on API or firehose paths.

Copy `.env.example`; all options are shown by `cargo run -- --help`. Analysis defaults are 1m/5m/1h/24h windows and a 30-second coordination interval.
When ingestion is catching up, coordination detection yields after
`ANALYSIS_MAX_LAG_SECONDS` (10 seconds by default) while archival, ClickHouse
storage, counters, and SSE delivery continue. Current upstream lag is exported as
`observatory_jetstream_lag_milliseconds`.

```bash
cargo test
docker compose config
docker compose logs -f observatory
curl -s localhost:3000/api/storage
curl -fsS localhost:3000/readyz
```

`fixtures/synthetic.jsonl` covers organic conversation, regular high-volume activity, synchronized reposts, normalized duplicate spam, and URL amplification. Tests cover parsing, normalization/fingerprints, archive/cursor behavior, coordination scoring, organic-burst resistance, and storage serialization.

System scope is explicit in the API: load, CPU utilization, and machine uptime are container-visible host kernel values; process and cgroup CPU percentages are sampled from their respective cumulative counters. Memory includes both this process and its cgroup where available, and array capacity is measured on the archive bind mount. ClickHouse reports active rows/parts and compressed versus uncompressed bytes per table, plus pending mutations, active queries, and server errors since restart.

## Account and relationship backfill

The optional `backfill_account` CLI reads one public repository through its PDS
using `describeRepo` and paginated `listRecords`. It normalizes every collection,
removes blob values, preserves valid record `createdAt` timestamps, and skips AT
URIs already in ClickHouse.

`backfill_associations` is a deliberately account-scoped second stage. It reads
only post URIs already retained for the selected DID, then queries the fixed
public Bluesky AppView for paginated likes, reposters and quotes plus a depth-one
thread view. It never crawls the repositories of interacting accounts. Progress
is atomically checkpointed on disk and deterministic event IDs make restarts
idempotent.

```bash
cargo run --release --bin backfill_associations -- \
  --did did:plc:example \
  --checkpoint /mnt/storage/bsky-observatory/backfill/example-associations.json
```

AppView's `getPostThread` response is not cursor-paginated. Consequently reply
coverage is bounded to the direct replies returned by a depth-one request and
must not be interpreted as an exhaustive reply history. `getRepostedBy` does
not expose the repost event timestamp, so those derived observations use the
time they were observed. Likes preserve `createdAt` (or `indexedAt`), while
quotes and replies preserve the post record's `createdAt` when valid. Media and
blob data are never retained.

## MVP boundaries

Hot state is process-local and rebuilds from new traffic; detections persist in ClickHouse. Growth rates are measured from the process-start disk-size baseline, so they read zero until new durable bytes arrive and reset after restart. Exact, normalized, and SimHash fingerprints are stored, while initial cluster generation uses repeated targets, normalized content and domains in temporal windows. Natural next steps are SimHash locality buckets, richer graph/community analysis, ClickHouse materialized rollups, and authentication. Semantic analysis should remain optional and limited to flagged/trending content.
