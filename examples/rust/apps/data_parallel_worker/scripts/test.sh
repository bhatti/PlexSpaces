#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for Data Parallel Worker App
# Starts Docker nodes, builds WASM, deploys to all nodes, and tests

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
# From examples/rust/apps/data_parallel_worker to workspace root: ../../../../ (4 levels)
WORKSPACE_ROOT="$(cd "$APP_DIR/../../../../" && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Data Parallel Worker App Test                                ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo

# Check Docker
if ! command -v docker &> /dev/null; then
    echo -e "${RED}❌ Docker not found${NC}"
    exit 1
fi

if ! docker info &> /dev/null; then
    echo -e "${RED}❌ Docker daemon not running${NC}"
    exit 1
fi

# Determine docker-compose command
if docker compose version &> /dev/null 2>&1; then
    DOCKER_COMPOSE="docker compose"
elif command -v docker-compose &> /dev/null; then
    DOCKER_COMPOSE="docker-compose"
else
    echo -e "${RED}❌ Docker Compose not found${NC}"
    exit 1
fi

echo -e "${BLUE}1. Building and starting Docker nodes (3-node cluster)...${NC}"
# Enable BuildKit for faster builds with cache mounts
export DOCKER_BUILDKIT=1
export COMPOSE_DOCKER_CLI_BUILD=1
# Use docker-compose from app directory (docker-compose file is there)
cd "$APP_DIR"

# Kill any existing plexspaces processes to avoid port conflicts
echo "Killing any existing plexspaces processes..."
pkill -9 -f 'plexspaces start' 2>/dev/null || true
pkill -9 -f 'firecracker_multi_tenant' 2>/dev/null || true
pkill -9 -f 'data_parallel_worker' 2>/dev/null || true
sleep 1

# Stop any existing containers first
$DOCKER_COMPOSE -f docker-compose.test.yml down 2>/dev/null || true

# Build only if needed (Docker will use cache if nothing changed)
# Use --no-cache only if explicitly needed for debugging
echo -e "${BLUE}   Building Docker images (using cache if available)...${NC}"
$DOCKER_COMPOSE -f docker-compose.test.yml build
echo -e "${GREEN}✓ Docker images built${NC}"
echo -e "${BLUE}   Starting containers...${NC}"
$DOCKER_COMPOSE -f docker-compose.test.yml up -d
echo -e "${GREEN}✓ Containers built and started${NC}"

# Wait for nodes to be ready - check both container status and port availability
echo -e "${BLUE}   Waiting for nodes to be ready...${NC}"
sleep 5  # Initial wait for containers to start

# Check container status first
for i in {1..30}; do
    if $DOCKER_COMPOSE -f docker-compose.test.yml ps | grep -q "Up"; then
        echo -e "${GREEN}✓ Containers are running${NC}"
        break
    fi
    if [ $i -eq 30 ]; then
        echo -e "${RED}✗ Containers failed to start${NC}"
        $DOCKER_COMPOSE -f docker-compose.test.yml ps
        $DOCKER_COMPOSE -f docker-compose.test.yml logs --tail=20 node1
        exit 1
    fi
    sleep 2
done

# Now wait for gRPC ports to be available (with timeout)
echo -e "${BLUE}   Waiting for gRPC ports to be ready...${NC}"
MAX_WAIT=60
for i in $(seq 1 $MAX_WAIT); do
    if nc -z localhost 8000 2>/dev/null && \
       nc -z localhost 8001 2>/dev/null && \
       nc -z localhost 8002 2>/dev/null; then
        echo -e "${GREEN}✓ All nodes ready (ports 8000, 8001, 8002)${NC}"
        break
    fi
    if [ $i -eq $MAX_WAIT ]; then
        echo -e "${YELLOW}⚠️  Nodes may not be fully ready after ${MAX_WAIT}s, checking status...${NC}"
        $DOCKER_COMPOSE -f docker-compose.test.yml ps
        echo -e "${YELLOW}⚠️  Continuing anyway...${NC}"
        break
    fi
    sleep 1
done

echo
echo -e "${BLUE}2. Building app (debug build)...${NC}"
cd "$APP_DIR"

# Use shared target directory
export CARGO_TARGET_DIR="$WORKSPACE_ROOT/target"

# Build the app (regular binary, not WASM - like session_manager)
if cargo build 2>&1 | tail -5 | grep -q "Finished\|Compiling"; then
    echo -e "${GREEN}✓ Build successful${NC}"
else
    echo -e "${RED}✗ Build failed${NC}"
    cargo build 2>&1 | tail -20
    exit 1
fi

BINARY_FILE="$WORKSPACE_ROOT/target/debug/data_parallel_worker"
if [ ! -f "$BINARY_FILE" ]; then
    echo -e "${RED}✗ Binary not found: $BINARY_FILE${NC}"
    exit 1
fi

echo
echo -e "${BLUE}3. Running test (data-parallel example)...${NC}"
export PLEXSPACES_NODE_ADDR=http://localhost:8000
export PLEXSPACES_NODE2_ADDR=http://localhost:8001
export PLEXSPACES_NODE3_ADDR=http://localhost:8002

# Run the test and capture exit code
set +e  # Don't exit on error, we'll check exit code manually
cargo run 2>&1 | tee /tmp/data-parallel-worker-test.log
EXIT_CODE=${PIPESTATUS[0]}
set -e  # Re-enable exit on error

if [ $EXIT_CODE -eq 0 ]; then
    # Check for error indicators in output even if exit code is 0
    if grep -q "❌\|Error:\|ERROR\|FATAL" /tmp/data-parallel-worker-test.log; then
        echo -e "${RED}✗ Test reported errors despite exit code 0${NC}"
        echo -e "${YELLOW}Error lines:${NC}"
        grep -E "❌|Error:|ERROR|FATAL" /tmp/data-parallel-worker-test.log | tail -10
        exit 1
    fi
    echo -e "${GREEN}✓ Test completed successfully${NC}"
else
    echo -e "${RED}✗ Test failed with exit code $EXIT_CODE${NC}"
    echo -e "${YELLOW}Last 30 lines of output:${NC}"
    tail -30 /tmp/data-parallel-worker-test.log
    exit 1
fi

echo
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✅ Test complete!${NC}"
echo
echo -e "${BLUE}To stop Docker nodes:${NC}"
echo "  cd $APP_DIR"
echo "  $DOCKER_COMPOSE -f docker-compose.test.yml down"
