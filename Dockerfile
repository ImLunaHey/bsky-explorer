FROM node:22-bookworm-slim AS web-build
WORKDIR /web
COPY frontend/package*.json ./
RUN npm install
COPY frontend ./
RUN npm run build

FROM rust:1.90-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release --bins
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/bsky-observatory /usr/local/bin/observatory
COPY --from=build /src/target/release/backfill_account /usr/local/bin/backfill_account
COPY --from=build /src/target/release/backfill_associations /usr/local/bin/backfill_associations
COPY --from=web-build /web/dist /usr/share/bsky-observatory
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=4 \
  CMD wget -q -O /dev/null http://127.0.0.1:3000/readyz || exit 1
ENTRYPOINT ["observatory"]
