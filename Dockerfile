FROM rust:1.90-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY static ./static
RUN cargo build --release
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/bsky-observatory /usr/local/bin/observatory
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=4 \
  CMD wget -q -O /dev/null http://127.0.0.1:3000/readyz || exit 1
ENTRYPOINT ["observatory"]
