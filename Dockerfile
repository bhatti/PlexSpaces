# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2026 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Multi-stage Dockerfile for PlexSpaces framework
# Framework-only container (Model 1: Dynamic WASM Deployment)
# Ready to accept WASM modules and actors via gRPC
# Shared base Dockerfile for all deployments

# Stage 1: Builder
# Pin Rust version: rust:1-bookworm currently provides 1.93.0 (matches rust-toolchain.toml)
# Install exact version via rustup for reproducibility
FROM rust:1-bookworm AS builder

# Install exact Rust version to match rust-toolchain.toml (1.93.0)
RUN rustup install 1.93.0 && rustup default 1.93.0

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
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

# Copy dependency files first (for better caching - cached unless Cargo.toml/Cargo.lock changes)
COPY Cargo.toml Cargo.lock ./

# Copy SDKs (workspace members required for cargo build, but not needed at runtime)
COPY sdks/ ./sdks/

# Copy all crates (includes pre-generated proto files in crates/proto/src/generated/)
# .dockerignore excludes examples/, tests/, etc. to minimize cache invalidation
# Proto-generated Rust files are checked into git, so no need to run buf generate
COPY crates/ ./crates/
COPY wit/ ./wit/

# Build arguments for features (dashboard is enabled by default via default features)
# Can override with --build-arg FEATURES="firecracker" or --build-arg FEATURES="dashboard,firecracker"
ARG FEATURES=""

# Build the plexspaces CLI binary (includes node start command)
# Use BuildKit cache mounts for Cargo registry, git cache, and target directory
# This dramatically speeds up rebuilds by caching dependencies and incremental compilation
# Docker will cache this layer unless source code or dependencies changed
# Dashboard is enabled by default, additional features can be specified via FEATURES
# Cache-busting: Force rebuild by touching a file (increment version to bust cache)
# Version: 2026-02-06-v1.0 (updated after fixing bindgen type resolution errors)
RUN echo "Build cache version: 2026-02-06-v1.0" > /tmp/build_version.txt && cat /tmp/build_version.txt

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    if [ -z "${FEATURES}" ]; then \
        cargo build --release -p plexspaces; \
    else \
        cargo build --release -p plexspaces --features "${FEATURES}"; \
    fi

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    curl \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

# Install grpc_health_probe for K8s health checks
RUN curl -sSL "https://github.com/grpc-ecosystem/grpc-health-probe/releases/download/v0.4.24/grpc_health_probe-linux-amd64" \
    -o "/usr/local/bin/grpc_health_probe" && \
    chmod +x /usr/local/bin/grpc_health_probe

# Create app user (non-root)
RUN useradd -m -u 1000 plexspaces

WORKDIR /app

# Copy the compiled binary from builder
COPY --from=builder /app/target/release/plexspaces /usr/local/bin/plexspaces

# Create config and data directories
# Data directory for SQLite databases and LocalFileSystem blob storage
RUN mkdir -p /app/config /app/data /app/data/blob /app/certs && \
    chown -R plexspaces:plexspaces /app

# Copy default release configuration (if exists)
COPY config/release.yaml /app/config/release.yaml 2>/dev/null || echo "# Default release config" > /app/config/release.yaml
RUN chown plexspaces:plexspaces /app/config/release.yaml

# Switch to non-root user
USER plexspaces

# Expose the default gRPC port
EXPOSE 8000

# Default environment variables (matching docs/installation.md)
# These can be overridden via docker-compose or Kubernetes
ENV PLEXSPACES_RELEASE_CONFIG=/app/config/release.yaml
ENV PLEXSPACES_NODE_ID=node1
ENV PLEXSPACES_LISTEN_ADDR=0.0.0.0:8000
ENV PLEXSPACES_BASE_DIR=/app/data

# Database configuration (defaults to SQLite file-based, don't override)
# Leave blank/default - config manager will use PLEXSPACES_BASE_DIR
# Default: sqlite://${base_dir}/db/plexspaces.db = sqlite:///app/data/db/plexspaces.db

# Backend configurations (defaults to SQLite file-based, don't override)
# Leave blank/default - config manager will use defaults from release.yaml
# Don't set :memory: - use file-based SQLite for persistence

# Blob storage configuration (defaults to LocalFileSystem, override in docker-compose)
# LocalFileSystem is easier for local development, docker-compose uses MinIO
# Leave blank/default - config manager will use defaults from release.yaml

# Health check using gRPC health probe
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD grpc_health_probe -addr=:8000 -service=readiness || nc -z localhost 8000 || exit 1

# Run the PlexSpaces node using CLI start command
# Framework starts empty, ready to accept deployments via gRPC
ENTRYPOINT ["plexspaces", "start"]
CMD ["--node-id", "${PLEXSPACES_NODE_ID}", "--listen-addr", "${PLEXSPACES_LISTEN_ADDR}", "--release-config", "${PLEXSPACES_RELEASE_CONFIG}"]
