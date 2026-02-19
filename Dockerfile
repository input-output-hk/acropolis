# syntax=docker/dockerfile:1.7

FROM rust:1.91-bookworm AS chef
WORKDIR /app
ENV CARGO_HOME=/cargo

RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY common ./common
COPY codec ./codec
COPY cardano ./cardano
COPY modules ./modules
COPY processes ./processes

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder-omnibus
ARG TARGETPLATFORM

COPY --from=planner /app/recipe.json /app/recipe.json

RUN --mount=type=cache,id=acropolis-cargo-registry-${TARGETPLATFORM},target=/cargo/registry \
    --mount=type=cache,id=acropolis-cargo-git-${TARGETPLATFORM},target=/cargo/git \
    cargo chef cook --locked --release --recipe-path /app/recipe.json --package acropolis_process_omnibus

COPY Cargo.toml Cargo.lock ./
COPY common ./common
COPY codec ./codec
COPY cardano ./cardano
COPY modules ./modules
COPY processes ./processes

RUN --mount=type=cache,id=acropolis-cargo-registry-${TARGETPLATFORM},target=/cargo/registry \
    --mount=type=cache,id=acropolis-cargo-git-${TARGETPLATFORM},target=/cargo/git \
    cargo build --locked --release --package acropolis_process_omnibus

FROM chef AS builder-midnight-indexer
ARG TARGETPLATFORM

COPY --from=planner /app/recipe.json /app/recipe.json

RUN --mount=type=cache,id=acropolis-cargo-registry-${TARGETPLATFORM},target=/cargo/registry \
    --mount=type=cache,id=acropolis-cargo-git-${TARGETPLATFORM},target=/cargo/git \
    cargo chef cook --locked --release --recipe-path /app/recipe.json --package acropolis_process_midnight_indexer

COPY Cargo.toml Cargo.lock ./
COPY common ./common
COPY codec ./codec
COPY cardano ./cardano
COPY modules ./modules
COPY processes ./processes

RUN --mount=type=cache,id=acropolis-cargo-registry-${TARGETPLATFORM},target=/cargo/registry \
    --mount=type=cache,id=acropolis-cargo-git-${TARGETPLATFORM},target=/cargo/git \
    cargo build --locked --release --package acropolis_process_midnight_indexer

FROM debian:bookworm-slim AS runtime-base

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
RUN groupadd --system --gid 10001 acropolis \
    && useradd --system --uid 10001 --gid 10001 --create-home --home-dir /home/acropolis acropolis

FROM runtime-base AS runtime-omnibus

WORKDIR /app/processes/omnibus

COPY --from=builder-omnibus /app/target/release/acropolis_process_omnibus /usr/local/bin/acropolis_process_omnibus
COPY --from=builder-omnibus /app/processes/omnibus/omnibus-preview.toml /app/processes/omnibus/omnibus-preview.toml
COPY --from=builder-omnibus /app/processes/omnibus/omnibus.toml /app/processes/omnibus/omnibus.toml
COPY --from=builder-omnibus /app/processes/omnibus/omnibus-local.toml /app/processes/omnibus/omnibus-local.toml
COPY --from=builder-omnibus /app/processes/omnibus/omnibus-rewards.toml /app/processes/omnibus/omnibus-rewards.toml
COPY --from=builder-omnibus /app/processes/omnibus/omnibus-sancho.toml /app/processes/omnibus/omnibus-sancho.toml
COPY --from=builder-omnibus /app/processes/omnibus/omnibus.bootstrap.toml /app/processes/omnibus/omnibus.bootstrap.toml
COPY --from=builder-omnibus /app/processes/omnibus/configs /app/processes/omnibus/configs
COPY --from=builder-omnibus /app/modules/accounts_state/test-data/pots.mainnet.csv /app/modules/accounts_state/test-data/pots.mainnet.csv

RUN mkdir -p /app/modules/mithril_snapshot_fetcher/downloads \
    /app/modules/snapshot_bootstrapper/data \
    /app/modules/address_state/db \
    /app/modules/historical_accounts_state/db \
    /app/processes/omnibus/fjall-spdd \
    && chown -R acropolis:acropolis /app /home/acropolis
USER acropolis:acropolis

EXPOSE 4340 4341

ENTRYPOINT ["acropolis_process_omnibus"]
CMD ["--config", "omnibus-preview.toml"]

FROM runtime-base AS runtime-midnight-indexer

WORKDIR /app/processes/midnight_indexer

COPY --from=builder-midnight-indexer /app/target/release/acropolis_process_midnight_indexer /usr/local/bin/acropolis_process_midnight_indexer
COPY --from=builder-midnight-indexer /app/processes/midnight_indexer/config.mainnet.toml /app/processes/midnight_indexer/config.mainnet.toml
COPY --from=builder-midnight-indexer /app/processes/midnight_indexer/config.preview.toml /app/processes/midnight_indexer/config.preview.toml

RUN mkdir -p /app/modules/mithril_snapshot_fetcher/downloads \
    /app/modules/snapshot_bootstrapper/data \
    /app/processes/midnight_indexer/fjall-spdd \
    /app/processes/midnight_indexer/fjall-blocks-mainnet \
    /app/processes/midnight_indexer/fjall-blocks-preview \
    && chown -R acropolis:acropolis /app /home/acropolis
USER acropolis:acropolis

ENTRYPOINT ["acropolis_process_midnight_indexer"]
CMD ["--config", "config.preview.toml"]
