#!/bin/bash

set -euo pipefail

echo "🔍 Running pre-commit checks..."

# Format code
echo "🎨 Formatting code..."
make format

# Lint proto files
echo "🔍 Linting proto files..."
make proto-lint

# Run clippy
echo "🔍 Running clippy..."
make lint

# Run tests
echo "🧪 Running tests..."
make test

# Check for security issues
echo "🔒 Running security audit..."
make audit

echo "✅ All pre-commit checks passed!"

---
# Dockerfile - Multi-stage Docker build
FROM rust:1.75-bullseye as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    cmake \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy dependency files
COPY Cargo.toml Cargo.lock ./
COPY src/bin/codegen.rs src/bin/
COPY build.rs ./

# Create dummy main to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --bin codegen
RUN rm src/main.rs

# Copy source code and proto files
COPY . .

# Generate code and build
RUN make deps generate build-release

# Runtime stage
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/plexspaces-server /usr/local/bin/
COPY --from=builder /app/target/release/plexspaces-client /usr/local/bin/

# Copy configuration files
COPY config/ ./config/

# Create non-root user
RUN useradd -r -s /bin/false plexspaces && \
    chown -R plexspaces:plexspaces /app

USER plexspaces

EXPOSE 50051 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD plexspaces-client health || exit 1

CMD ["plexspaces-server"]

