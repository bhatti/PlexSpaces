# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Multi-stage Dockerfile for PlexSpaces framework
# Framework-only container (Model 1: Dynamic WASM Deployment)
# Ready to accept WASM modules and actors via gRPC
# Shared base Dockerfile for all deployments
#
# Uses cargo-chef to cache dependency compilation as a separate layer.
# Only changed crates recompile on rebuild; the dependency layer stays cached
# unless Cargo.toml / Cargo.lock change.

# ─── Stage 0: cargo-chef planner ────────────────────────────────────────────
FROM rust:1.93.0-bookworm AS chef

RUN cargo install cargo-chef --version 0.1.71 --locked

WORKDIR /app

# ─── Stage 1: Compute the build recipe ───────────────────────────────────────
FROM chef AS planner

# Copy everything cargo-chef needs to fingerprint the dependency graph.
# .dockerignore keeps examples/, target/, .git/, docs/, etc. out of context.
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY src/ ./src/
COPY db/ ./db/
COPY crates/ ./crates/
COPY sdks/ ./sdks/
COPY wit/ ./wit/

RUN cargo chef prepare --recipe-path recipe.json

# ─── Stage 2: Pre-build ALL dependencies (cached unless deps change) ─────────
FROM chef AS builder

# Install build-time system libraries once; layer is cached.
# clang/llvm/libclang-dev are required by wasmtime's cranelift backend.
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    cmake \
    build-essential \
    clang \
    llvm \
    libclang-dev \
    git \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=planner /app/recipe.json recipe.json

# Build arguments for optional features
ARG FEATURES=""
ARG ENABLE_FIRECRACKER="0"

# This layer is ~95% of total build time and is fully cached until
# Cargo.toml / Cargo.lock change.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    feature_args="--features plexspaces-node/dashboard"; \
    if [ -n "${FEATURES}" ]; then \
        feature_args="${feature_args} --features ${FEATURES}"; \
    fi; \
    if [ "${ENABLE_FIRECRACKER}" = "1" ]; then \
        feature_args="${feature_args} --features firecracker --features plexspaces-node/firecracker"; \
    fi; \
    cargo chef cook --release -p plexspaces-cli ${feature_args} --recipe-path recipe.json

# ─── Stage 3: Build application code ─────────────────────────────────────────
# Only this stage re-runs when source files change.
COPY Cargo.toml Cargo.lock ./
COPY build.rs ./
COPY src/ ./src/
COPY db/ ./db/
COPY crates/ ./crates/
COPY sdks/ ./sdks/
COPY wit/ ./wit/

RUN mkdir -p ./config
COPY release.yaml ./config/release.yaml

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    feature_args="--features plexspaces-node/dashboard"; \
    if [ -n "${FEATURES}" ]; then \
        feature_args="${feature_args} --features ${FEATURES}"; \
    fi; \
    if [ "${ENABLE_FIRECRACKER}" = "1" ]; then \
        feature_args="${feature_args} --features firecracker --features plexspaces-node/firecracker"; \
    fi; \
    cargo build --release -p plexspaces-cli ${feature_args} && \
    cp /app/target/release/plexspaces /tmp/plexspaces

# ─── Stage 4: Runtime image ───────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 \
    ca-certificates \
    curl \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Install grpc_health_probe for K8s health checks
RUN curl -sSL "https://github.com/grpc-ecosystem/grpc-health-probe/releases/download/v0.4.24/grpc_health_probe-linux-amd64" \
    -o "/usr/local/bin/grpc_health_probe" && \
    chmod +x /usr/local/bin/grpc_health_probe

RUN useradd -m -u 1000 plexspaces

WORKDIR /app

COPY --from=builder /tmp/plexspaces /usr/local/bin/plexspaces

RUN mkdir -p /app/config /app/data /app/data/blob /app/certs && \
    chown -R plexspaces:plexspaces /app

COPY --from=builder /app/config/release.yaml /app/config/release.yaml
RUN chown plexspaces:plexspaces /app/config/release.yaml

USER plexspaces

EXPOSE 8000

ENV PLEXSPACES_RELEASE_CONFIG=/app/config/release.yaml
ENV PLEXSPACES_NODE_ID=node1
ENV PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000
ENV PLEXSPACES_BASE_DIR=/app/data

HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD grpc_health_probe -addr=:8000 -service=readiness || nc -z localhost 8000 || exit 1

ENTRYPOINT ["plexspaces", "start"]
CMD ["--node-id", "node1", "--listen-addr", "0.0.0.0:8000", "--release-config", "/app/config/release.yaml"]
