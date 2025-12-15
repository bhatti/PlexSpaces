#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Script to run Firecracker integration tests
# Requires Docker and docker-compose

set -e

echo "🔥 Firecracker Integration Tests"
echo "================================"

# Check Docker
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed"
    exit 1
fi

# Check for docker-compose or docker compose (newer Docker versions)
DOCKER_COMPOSE_CMD=""
if command -v docker-compose &> /dev/null; then
    DOCKER_COMPOSE_CMD="docker-compose"
    echo "✓ Using docker-compose (standalone)"
elif docker compose version &> /dev/null 2>&1; then
    DOCKER_COMPOSE_CMD="docker compose"
    echo "✓ Using docker compose (plugin)"
else
    echo "❌ docker-compose is not installed"
    echo "   Please install docker-compose or use Docker with compose plugin"
    exit 1
fi

# Check if running on Linux (Firecracker requires KVM)
if [[ "$OSTYPE" != "linux-gnu"* ]]; then
    echo "⚠️  Warning: Firecracker requires Linux with KVM support"
    echo "   Tests will be marked as ignored on non-Linux systems"
fi

# Build Docker image
echo ""
echo "📦 Building Docker image..."
if [[ "$DOCKER_COMPOSE_CMD" == "docker compose" ]]; then
    docker compose -f docker-compose.firecracker-test.yml build
else
    docker-compose -f docker-compose.firecracker-test.yml build
fi

# Run tests
echo ""
echo "🧪 Running integration tests..."
cargo test -p plexspaces-node --test firecracker_integration_test --features firecracker -- --ignored

# Cleanup
echo ""
echo "🧹 Cleaning up..."
if [[ "$DOCKER_COMPOSE_CMD" == "docker compose" ]]; then
    docker compose -f docker-compose.firecracker-test.yml down
else
    docker-compose -f docker-compose.firecracker-test.yml down
fi

echo ""
echo "✅ Integration tests complete!"
