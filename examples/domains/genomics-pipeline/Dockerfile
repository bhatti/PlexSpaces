# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Dockerfile for Genomics Pipeline Node (Runtime-only)
#
# PREREQUISITE: Build the binary first with:
#   cd /Users/sbhatti/workspace/PlexSpaces/examples/genomics-pipeline
#   cargo build --release --bin genomics-node

FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -m -u 1000 plexspaces

# Copy pre-built binary from host
COPY target/release/genomics-node /usr/local/bin/genomics-node

# Copy config directory (if it exists)
# COPY config /config

# Set working directory
WORKDIR /app

# Switch to app user
USER plexspaces

# Health check
HEALTHCHECK --interval=10s --timeout=5s --start-period=30s --retries=3 \
  CMD [ "sh", "-c", "curl -f http://localhost:9001/health || exit 1" ]

# Default command
ENTRYPOINT ["genomics-node"]
CMD ["--config", "/config/node1.toml"]
