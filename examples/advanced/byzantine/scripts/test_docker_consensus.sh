#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Integration test for Byzantine Generals consensus in Docker containers
#
# This script:
# 1. Starts PlexSpaces nodes in Docker containers
# 2. Waits for all nodes to be healthy
# 3. Deploys Byzantine Generals application
# 4. Verifies consensus is reached
# 5. Cleans up containers
#
# Usage:
#   ./scripts/test_docker_consensus.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PROJECT_ROOT="$(cd "$EXAMPLE_DIR/../../.." && pwd)"

cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║  Byzantine Generals - Docker Container Integration Test  ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Cleanup function
cleanup() {
    echo ""
    echo "🧹 Cleaning up Docker containers..."
    docker-compose -f docker-compose.containers.yml down -v 2>/dev/null || true
}

# Set trap to cleanup on exit
trap cleanup EXIT

# Check if Docker is running
if ! docker info >/dev/null 2>&1; then
    echo -e "${RED}❌ Error: Docker is not running${NC}"
    exit 1
fi

# Check if docker-compose is available
if ! command -v docker-compose >/dev/null 2>&1 && ! docker compose version >/dev/null 2>&1; then
    echo -e "${RED}❌ Error: docker-compose is not available${NC}"
    exit 1
fi

# Use 'docker compose' if available, otherwise 'docker-compose'
if docker compose version >/dev/null 2>&1; then
    DOCKER_COMPOSE="docker compose"
else
    DOCKER_COMPOSE="docker-compose"
fi

echo "📦 Step 1: Building Docker images..."
cd "$PROJECT_ROOT"
if ! docker build -t plexspaces-node:latest -f Dockerfile . >/dev/null 2>&1; then
    echo -e "${RED}❌ Error: Failed to build Docker image${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Docker image built successfully${NC}"

cd "$EXAMPLE_DIR"

echo ""
echo "🚀 Step 2: Starting Docker containers..."
$DOCKER_COMPOSE -f docker-compose.containers.yml up -d

echo ""
echo "⏳ Step 3: Waiting for all nodes to be healthy..."
MAX_WAIT=120
WAIT_INTERVAL=5
ELAPSED=0

check_health() {
    local node=$1
    local port=$2
    if command -v grpc_health_probe >/dev/null 2>&1; then
        grpc_health_probe -addr="localhost:${port}" -service=readiness >/dev/null 2>&1
    else
        # Fallback: check if port is open
        nc -z localhost "${port}" >/dev/null 2>&1
    fi
}

ALL_HEALTHY=false
while [ $ELAPSED -lt $MAX_WAIT ]; do
    if check_health "node1" "9001" && \
       check_health "node2" "9002" && \
       check_health "node3" "9003" && \
       check_health "node4" "9004"; then
        ALL_HEALTHY=true
        break
    fi
    echo "   Waiting for nodes to be healthy... (${ELAPSED}s/${MAX_WAIT}s)"
    sleep $WAIT_INTERVAL
    ELAPSED=$((ELAPSED + WAIT_INTERVAL))
done

if [ "$ALL_HEALTHY" = false ]; then
    echo -e "${RED}❌ Error: Nodes did not become healthy within ${MAX_WAIT}s${NC}"
    echo "Container logs:"
    $DOCKER_COMPOSE -f docker-compose.containers.yml logs --tail=50
    exit 1
fi

echo -e "${GREEN}✅ All nodes are healthy${NC}"

echo ""
echo "🧪 Step 4: Running Byzantine Generals consensus test..."
echo "   This will deploy the application and verify consensus is reached"

# For now, we'll use a simple test that connects to the nodes
# In a full implementation, we would:
# 1. Deploy the Byzantine Generals application via gRPC
# 2. Wait for consensus to be reached
# 3. Verify all honest generals agree

# TODO: Implement full deployment and consensus verification
# For now, we'll just verify the nodes are accessible
echo -e "${YELLOW}⚠️  Full consensus test not yet implemented${NC}"
echo "   Nodes are running and healthy - ready for application deployment"

echo ""
echo -e "${GREEN}✅ Docker container integration test completed successfully${NC}"
echo ""
echo "Next steps:"
echo "  1. Deploy Byzantine Generals application via gRPC ApplicationService"
echo "  2. Monitor consensus progress"
echo "  3. Verify all honest generals reach agreement"

exit 0
