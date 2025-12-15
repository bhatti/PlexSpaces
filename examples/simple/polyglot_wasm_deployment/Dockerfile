# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Dockerfile for Polyglot WASM Deployment Example
# Simple Docker image for testing node deployment and application deployment

FROM rust:1-bookworm AS builder

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

# Copy workspace files for dependency resolution
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY proto/ ./proto/
COPY wit/ ./wit/
# Copy example Cargo.toml (but NOT Cargo.lock - use workspace lock file)
COPY examples/simple/polyglot_wasm_deployment/Cargo.toml ./examples/simple/polyglot_wasm_deployment/

# Dummy build to cache dependencies (skip if it fails - proto files may need generation)
RUN mkdir -p examples/simple/polyglot_wasm_deployment/src && \
    echo "fn main() {}" > examples/simple/polyglot_wasm_deployment/src/main.rs && \
    cargo build --release -p polyglot-wasm-deployment 2>&1 | head -20 || true && \
    rm -rf examples/simple/polyglot_wasm_deployment/src

# Copy the rest of the application code (excluding Cargo.lock if it exists)
COPY examples/simple/polyglot_wasm_deployment/ ./examples/simple/polyglot_wasm_deployment/

# Remove local Cargo.lock if it exists (use workspace Cargo.lock instead)
RUN rm -f ./examples/simple/polyglot_wasm_deployment/Cargo.lock

# Build the example from workspace root
WORKDIR /app
RUN cd examples/simple/polyglot_wasm_deployment && cargo build --release

# Runtime stage
FROM debian:bookworm

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy built binary
COPY --from=builder /app/examples/simple/polyglot_wasm_deployment/target/release/polyglot_wasm_deployment /usr/local/bin/

WORKDIR /app
COPY --from=builder /app/examples/simple/polyglot_wasm_deployment/release.toml ./

# Create directory for temporary files
RUN mkdir -p /tmp && chmod 1777 /tmp

# Expose gRPC port for ApplicationService
EXPOSE 9000

CMD ["polyglot_wasm_deployment", "start-node", "--node-id", "docker-node", "--address", "0.0.0.0:9000"]

