# Multi-stage build for genomic-workflow-pipeline
# Use latest to get edition2024 support
FROM rust:latest as builder

WORKDIR /build

# Copy workspace files
COPY Cargo.toml ./
COPY crates ./crates
COPY src ./src

# Copy example
COPY examples/genomic-workflow-pipeline ./examples/genomic-workflow-pipeline

# Build the example (use --locked to respect Cargo.lock)
RUN cd examples/genomic-workflow-pipeline && \
    cargo build --release --locked

# Runtime image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy binary
COPY --from=builder /build/examples/genomic-workflow-pipeline/target/release/genomic-workflow-pipeline /usr/local/bin/

# Set working directory
WORKDIR /app

# Default command with 10K reads
CMD ["genomic-workflow-pipeline", "10000"]
