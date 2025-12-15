# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Multi-stage Dockerfile for PlexSpaces framework
# Framework-only container (Model 1: Dynamic WASM Deployment)
# Ready to accept WASM modules and actors via gRPC

# Stage 1: Builder
FROM rust:1.75-slim-bookworm AS builder

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

# Install buf for proto linting and generation
RUN curl -sSL "https://github.com/bufbuild/buf/releases/latest/download/buf-Linux-x86_64" \
    -o "/usr/local/bin/buf" && \
    chmod +x /usr/local/bin/buf

# Copy workspace files for dependency resolution
COPY Cargo.toml Cargo.lock ./
COPY crates/proto/Cargo.toml ./crates/proto/
COPY crates/node/Cargo.toml ./crates/node/

# Dummy build to cache dependencies
RUN mkdir -p src crates/proto/src crates/node/src && \
    echo "fn main() {}" > src/main.rs && \
    echo "// dummy" > crates/proto/src/lib.rs && \
    echo "// dummy" > crates/node/src/lib.rs && \
    cargo build --release -p plexspaces-node || true && \
    rm -rf src crates/proto/src crates/node/src

# Copy the rest of the application code
COPY . .

# Generate proto files
RUN make proto || true

# Build the plexspaces-node binary (framework runtime)
RUN cargo build --release -p plexspaces-node

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Install grpc_health_probe for K8s health checks
RUN curl -sSL "https://github.com/grpc-ecosystem/grpc-health-probe/releases/download/v0.4.24/grpc_health_probe-linux-amd64" \
    -o "/usr/local/bin/grpc_health_probe" && \
    chmod +x /usr/local/bin/grpc_health_probe

# Create app user (non-root)
RUN useradd -m -u 1000 plexspaces

WORKDIR /app

# Copy the compiled binary from builder
COPY --from=builder /app/target/release/plexspaces-node ./plexspaces-node

# Create config and data directories
RUN mkdir -p /app/config /app/data && chown -R plexspaces:plexspaces /app

# Switch to non-root user
USER plexspaces

# Expose the default gRPC port
EXPOSE 9001

# Default environment variables for SQLite backends
ENV PLEXSPACES_JOURNALING_BACKEND=sqlite
ENV PLEXSPACES_JOURNALING_SQLITE_PATH=/app/data/journal.db
ENV PLEXSPACES_CHANNEL_BACKEND=sqlite
ENV PLEXSPACES_CHANNEL_SQLITE_PATH=/app/data/channel.db
ENV PLEXSPACES_TUPLESPACE_BACKEND=sqlite
ENV PLEXSPACES_TUPLESPACE_SQLITE_PATH=/app/data/tuplespace.db

# Health check using gRPC health probe
# Note: Requires grpc_health_probe binary (install separately or use HTTP via gRPC-Gateway)
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD grpc_health_probe -addr=:9001 -service=readiness || exit 1

# Run the PlexSpaces framework node
# Framework starts empty, ready to accept WASM deployments via gRPC
ENTRYPOINT ["./plexspaces-node"]
CMD ["--config", "/app/config/node.toml"]

