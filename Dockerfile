# syntax=docker/dockerfile:1.7

FROM rust:1.91-bookworm AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY common ./common
COPY codec ./codec
COPY cardano ./cardano
COPY modules ./modules
COPY processes ./processes

RUN cargo build --release -p acropolis_process_omnibus

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app/processes/omnibus

COPY --from=builder /app/target/release/acropolis_process_omnibus /usr/local/bin/acropolis_process_omnibus
COPY --from=builder /app/processes/omnibus /app/processes/omnibus
COPY --from=builder /app/modules/snapshot_bootstrapper/data /app/modules/snapshot_bootstrapper/data

RUN mkdir -p /app/processes/omnibus/downloads

EXPOSE 4340 4341

ENTRYPOINT ["acropolis_process_omnibus"]
CMD ["--config", "omnibus-preview.toml"]
