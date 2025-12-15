# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Dockerfile for Finance Risk Assessment Node (Multi-stage build)
#
# Builds the binary inside Docker for correct Linux architecture

# Stage 1: Build
FROM rustlang/rust:nightly-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /workspace

# Copy PlexSpaces workspace root files and directories
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY proto ./proto
COPY examples/finance-risk ./examples/finance-risk

# Build the finance-node binary from workspace root
RUN cd examples/finance-risk && cargo build --release --bin finance-node

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1000 plexspaces

# Copy binary from builder stage
COPY --from=builder /workspace/examples/finance-risk/target/release/finance-node /usr/local/bin/finance-node

# Copy entrypoint script
COPY examples/finance-risk/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Copy config directory (if it exists)
# COPY config /config

# Create data directory for SQLite databases
RUN mkdir -p /data/finance && chmod 755 /data/finance

# Set working directory
WORKDIR /app

# Health check
HEALTHCHECK --interval=10s --timeout=5s --start-period=30s --retries=3 \
  CMD [ "sh", "-c", "curl -f http://localhost:9101/health || exit 1" ]

# Use entrypoint script to fix volume permissions before starting finance-node
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["finance-node", "--config", "/config/node1.toml"]
