# Bluesky Network Observatory MVP

A single-machine Rust + ClickHouse observatory for public ATProto activity. It connects to Bluesky Jetstream from startup, archives the original JSON with Zstandard compression, normalizes non-blob records, maintains rolling hot state, and emits evidence-led coordination scores. It never downloads media/profile blobs and never calls an LLM.

## Run on the storage array

Compose bind-mounts every durable byte under `/mnt/storage/bsky-observatory`. On this host the array has about 3.5 TB available, but the current account cannot create its application directory. Run once as an administrator:

```bash
sudo install -d -o "$USER" -g "$(id -gn)" /mnt/storage/bsky-observatory/{archive,clickhouse}
cp .env.example .env
docker compose up --build -d
```

Open <http://localhost:3000>. Prometheus metrics are at `/metrics`; JSON endpoints are under `/api`. Set `STORAGE_ROOT` for another array mount. Do not point it at this repository. Compose passes all retention, batching, window, and ClickHouse credential settings from `.env` into the service; the storage array remains the only durable-data location.

## Data and restart behavior

Jetstream JSON is flushed into rotating `.jsonl.zst` files before the microsecond cursor is atomically advanced. Reconnects request that cursor; a duplicate boundary is possible and preferable to upstream loss. The deterministic `event_id` supports future deduplication. Archives are the recovery/reprocessing source if ClickHouse is unavailable, while the bounded ingest channel applies backpressure.

Archive segments can be validated and reprocessed with the replay binary. It defaults to a read-only validation pass; add `--write` to batch normalized rows into ClickHouse. Replay snapshots segments in filename order, fails with file/line context on corrupt input, and never reads or advances the live Jetstream cursor. Stop ingestion or replay only completed/copied segments if the newest archive file is still being written. Replaying overlapping ranges may create duplicate physical rows, so use time bounds deliberately.

```bash
cargo run --bin replay -- --archive-dir /mnt/storage/bsky-observatory/archive
cargo run --bin replay -- --archive-dir /mnt/storage/bsky-observatory/archive \
  --since-time-us 1789060000000000 --until-time-us 1789063600000000 --write
```

ClickHouse uses monthly `MergeTree` partitions, time/kind/actor ordering, Zstd text/evidence compression, arrays for URLs/domains, and skip indexes. Raw records and normalized rows default to 30 and 365 day retention respectively. No schema column stores a blob.

## API

- `GET /api/stats` — event/post/like/follow/interaction rates
- `GET /api/trending/urls` and `/api/trending/domains`
- `GET /api/anomalies`, `/api/clusters`, `/api/clusters/{id}`
- `GET /api/accounts/{did}` — rolling activity and automation evidence
- `GET /api/storage` — archive, ClickHouse/derived sizes and observed growth projections
- `GET /metrics` — Prometheus exposition
- `GET /healthz` — process liveness
- `GET /readyz` — readiness, including a ClickHouse query

Scores describe observed evidence (`automation`, coordination, shared-target, timing, and content similarity); they do not label accounts as bots. Cluster evidence includes participants, timing, shared targets, URLs/domains, and sample event IDs.

## Configuration and verification

Copy `.env.example`; all options are shown by `cargo run -- --help`. Analysis defaults are 1m/5m/1h/24h windows and a 30-second coordination interval.

```bash
cargo test
docker compose config
docker compose logs -f observatory
curl -s localhost:3000/api/storage
curl -fsS localhost:3000/readyz
```

`fixtures/synthetic.jsonl` covers organic conversation, regular high-volume activity, synchronized reposts, normalized duplicate spam, and URL amplification. Tests cover parsing, normalization/fingerprints, archive/cursor behavior, coordination scoring, organic-burst resistance, and storage serialization.

## MVP boundaries

Hot state is process-local and rebuilds from new traffic; detections persist in ClickHouse. Growth rates are measured from the process-start disk-size baseline, so they read zero until new durable bytes arrive and reset after restart. Exact, normalized, and SimHash fingerprints are stored, while initial cluster generation uses repeated targets, normalized content and domains in temporal windows. Natural next steps are SimHash locality buckets, richer graph/community analysis, ClickHouse materialized rollups, and authentication. Semantic analysis should remain optional and limited to flagged/trending content.
