# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Dockerfile for Firecracker Multi-Tenant Example
# Includes Firecracker binary, kernel, and rootfs for testing

# Build for linux/amd64 (Firecracker only supports x86_64)
# Use rust:1-bookworm to match Debian bookworm base (ensures GLIBC compatibility)
FROM --platform=linux/amd64 rust:1-bookworm AS builder

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
    wget \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files for dependency resolution
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY proto/ ./proto/
COPY wit/ ./wit/
# Copy example Cargo.toml (but NOT Cargo.lock - use workspace lock file)
COPY examples/simple/firecracker_multi_tenant/Cargo.toml ./examples/simple/firecracker_multi_tenant/

# Dummy build to cache dependencies (skip if it fails - proto files may need generation)
RUN mkdir -p examples/simple/firecracker_multi_tenant/src && \
    echo "fn main() {}" > examples/simple/firecracker_multi_tenant/src/main.rs && \
    cargo build --release -p firecracker-multi-tenant 2>&1 | head -20 || true && \
    rm -rf examples/simple/firecracker_multi_tenant/src

# Copy the rest of the application code (excluding Cargo.lock if it exists)
COPY examples/simple/firecracker_multi_tenant/ ./examples/simple/firecracker_multi_tenant/

# Remove local Cargo.lock if it exists (use workspace Cargo.lock instead)
RUN rm -f ./examples/simple/firecracker_multi_tenant/Cargo.lock

# Build the example from workspace root
WORKDIR /app
RUN cd examples/simple/firecracker_multi_tenant && cargo build --release

# Runtime stage - must be linux/amd64 for Firecracker
# Use debian:bookworm to match rust:1-bookworm builder (ensures GLIBC compatibility)
FROM --platform=linux/amd64 debian:bookworm

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    wget \
    && rm -rf /var/lib/apt/lists/*

# Install Firecracker
RUN mkdir -p /usr/bin /var/lib/firecracker && \
    wget -q https://github.com/firecracker-microvm/firecracker/releases/download/v1.4.0/firecracker-v1.4.0-x86_64.tgz && \
    tar -xzf firecracker-v1.4.0-x86_64.tgz && \
    mv release-v1.4.0-x86_64/firecracker-v1.4.0-x86_64 /usr/bin/firecracker && \
    chmod +x /usr/bin/firecracker && \
    rm -rf firecracker-v1.4.0-x86_64.tgz release-v1.4.0-x86_64

# Download kernel image
RUN wget -q https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin -O /var/lib/firecracker/vmlinux && \
    chmod 644 /var/lib/firecracker/vmlinux

# Download rootfs image
RUN wget -q https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4 -O /var/lib/firecracker/rootfs.ext4 && \
    chmod 644 /var/lib/firecracker/rootfs.ext4

# Copy built binary
COPY --from=builder /app/examples/simple/firecracker_multi_tenant/target/release/firecracker_multi_tenant /usr/local/bin/

WORKDIR /app
COPY --from=builder /app/examples/simple/firecracker_multi_tenant/release.toml ./

# Create directory for Firecracker sockets
RUN mkdir -p /tmp && chmod 1777 /tmp

# Verify Firecracker installation
RUN firecracker --version

CMD ["firecracker_multi_tenant", "list"]

