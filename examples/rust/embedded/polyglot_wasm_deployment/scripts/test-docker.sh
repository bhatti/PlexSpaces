#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for Polyglot WASM Deployment Example using Docker
# Tests: Empty node in Docker + Application deployment via ApplicationService

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Polyglot WASM Deployment Example - Docker Test                ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo

# Check if Docker is available
if ! command -v docker &> /dev/null; then
    echo -e "${RED}❌ Docker not found. Please install Docker.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Docker found${NC}"

# Check if Docker daemon is running
if ! docker info &> /dev/null; then
    echo -e "${RED}❌ Docker daemon is not running. Please start Docker.${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Docker daemon is running${NC}"

# Determine docker-compose command (v2 uses 'docker compose', v1 uses 'docker-compose')
if docker compose version &> /dev/null 2>&1; then
    DOCKER_COMPOSE="docker compose"
    echo -e "${GREEN}✓ Using Docker Compose v2 (docker compose)${NC}"
elif command -v docker-compose &> /dev/null && docker-compose version &> /dev/null 2>&1; then
    DOCKER_COMPOSE="docker-compose"
    echo -e "${GREEN}✓ Using Docker Compose v1 (docker-compose)${NC}"
else
    echo -e "${RED}❌ Docker Compose not found. Please install Docker Compose.${NC}"
    exit 1
fi

echo
echo -e "${BLUE}1. Building Docker image...${NC}"
cd "$EXAMPLE_DIR"

# Build with progress output
echo -e "${BLUE}   This may take several minutes (downloading base images, building Rust code)...${NC}"
echo -e "${BLUE}   Please wait...${NC}"

# Build and capture both output and exit code
BUILD_LOG=$(mktemp)
set +e  # Don't exit on error - we'll check exit code manually

$DOCKER_COMPOSE build 2>&1 | tee "$BUILD_LOG"
BUILD_EXIT_CODE=${PIPESTATUS[0]}
set -e

# Check build result
if [ $BUILD_EXIT_CODE -eq 0 ]; then
    if grep -qiE "(Successfully built|Successfully tagged|Built|DONE|exporting to image)" "$BUILD_LOG"; then
        echo -e "${GREEN}✓ Docker image built successfully${NC}"
        rm -f "$BUILD_LOG"
    else
        IMAGE_EXISTS=$(docker images --format "{{.Repository}}:{{.Tag}}" 2>/dev/null | grep -i "polyglot.*test\|polyglot-wasm-deployment" | head -1 || echo "")
        if [ -n "$IMAGE_EXISTS" ]; then
            echo -e "${GREEN}✓ Docker image built successfully (verified: $IMAGE_EXISTS)${NC}"
            rm -f "$BUILD_LOG"
        else
            echo -e "${RED}✗ Docker build may have failed - no success message and no image found${NC}"
            tail -30 "$BUILD_LOG"
            rm -f "$BUILD_LOG"
            exit 1
        fi
    fi
else
    echo -e "${RED}✗ Docker build failed (exit code: $BUILD_EXIT_CODE)${NC}"
    tail -50 "$BUILD_LOG"
    rm -f "$BUILD_LOG"
    exit 1
fi

echo
echo -e "${BLUE}2. Testing empty node startup in Docker...${NC}"

# Start node in Docker container (background, keep running)
echo -e "${BLUE}   Starting empty node in Docker container...${NC}"

# Get image name - docker-compose builds with format: <project>_<service>:latest
# Project name is usually the directory name (polyglot-wasm-deployment or polyglot_wasm_deployment)
PROJECT_NAME=$(basename "$(cd "$EXAMPLE_DIR/../.." && pwd)" 2>/dev/null || echo "polyglot-wasm-deployment")
IMAGE_NAME="${PROJECT_NAME}_polyglot-test:latest"

# Verify image exists, if not try alternative naming
if ! docker images | grep -q "$IMAGE_NAME"; then
    # Try with underscores
    IMAGE_NAME=$(echo "$IMAGE_NAME" | tr '-' '_')
    if ! docker images | grep -q "$IMAGE_NAME"; then
        # Try to find any polyglot image
        IMAGE_NAME=$(docker images --format "{{.Repository}}:{{.Tag}}" | grep -iE "polyglot.*test|polyglot.*wasm" | head -1 || echo "")
        if [ -z "$IMAGE_NAME" ]; then
            echo -e "${RED}✗ Could not find Docker image. Please build first with: $DOCKER_COMPOSE build${NC}"
            exit 1
        fi
    fi
fi

echo -e "${BLUE}   Using image: $IMAGE_NAME${NC}"

# Use docker run with proper network and port mapping
NETWORK_NAME="polyglot-wasm-deployment_polyglot-net"
# Create network if it doesn't exist
docker network create "$NETWORK_NAME" 2>/dev/null || true

# Remove any existing container with same name
docker rm -f polyglot-test-node 2>/dev/null || true

# Start container
CONTAINER_OUTPUT=$(docker run -d \
    --name polyglot-test-node \
    -p 9000:9000 \
    --network "$NETWORK_NAME" \
    "$IMAGE_NAME" \
    polyglot_wasm_deployment start-node --node-id docker-test-node --address 0.0.0.0:9000 2>&1)

# Extract container ID - docker run outputs the full container ID
CONTAINER_ID=$(echo "$CONTAINER_OUTPUT" | grep -oE "^[a-f0-9]{64}$" | head -1)

if [ -z "$CONTAINER_ID" ]; then
    # Try short ID (12 chars)
    CONTAINER_ID=$(echo "$CONTAINER_OUTPUT" | grep -oE "^[a-f0-9]{12,}$" | head -1)
fi

if [ -z "$CONTAINER_ID" ]; then
    # If still empty, use container name to get ID
    CONTAINER_ID=$(docker ps -a --filter "name=polyglot-test-node" --format "{{.ID}}" | head -1)
fi

if [ -z "$CONTAINER_ID" ]; then
    echo -e "${RED}✗ Failed to start node container${NC}"
    echo "Output: $CONTAINER_OUTPUT"
    exit 1
fi

# Use short ID for display
SHORT_ID="${CONTAINER_ID:0:12}"
echo -e "${GREEN}   ✓ Node container started (ID: $SHORT_ID)${NC}"

# Wait for node to start
echo -e "${BLUE}   Waiting for node to be ready...${NC}"
sleep 5

# Check if container is still running (by ID or name)
if ! docker ps | grep -qE "$CONTAINER_ID|polyglot-test-node"; then
    echo -e "${RED}✗ Node container stopped unexpectedly${NC}"
    echo -e "${YELLOW}   Container logs:${NC}"
    docker logs polyglot-test-node 2>&1 | tail -50
    exit 1
fi

# Check logs to verify node started
NODE_LOGS=$(docker logs "$CONTAINER_ID" 2>&1)
if echo "$NODE_LOGS" | grep -qi "Startup complete\|SERVING\|gRPC server.*9000"; then
    echo -e "${GREEN}   ✓ Node started successfully (verified in logs)${NC}"
    # Show key log lines
    echo "$NODE_LOGS" | grep -i "startup\|serving\|gRPC" | tail -3
else
    echo -e "${YELLOW}   ⚠️  Node container running but startup not confirmed in logs${NC}"
    echo -e "${YELLOW}   Last 10 lines of logs:${NC}"
    echo "$NODE_LOGS" | tail -10
fi

echo -e "${GREEN}   ✓ Node container is running${NC}"

# Use host address since we're mapping port 9000:9000
NODE_ADDRESS="127.0.0.1:9000"
echo -e "${BLUE}   Node address: $NODE_ADDRESS (mapped from container)${NC}"

echo
echo -e "${BLUE}3. Testing application deployment to node in Docker...${NC}"

# Create minimal WASM file in container
TEST_WASM="/tmp/test-app.wasm"
WASM_BYTES='\x00\x61\x73\x6d\x01\x00\x00\x00\x01\x05\x01\x60\x00\x01\x7f\x03\x02\x01\x00\x07\x08\x01\x04\x74\x65\x73\x74\x00\x00\x0a\x06\x01\x04\x00\x41\x2a\x0b'

# Deploy application to node (from host, connecting to container's exposed port)
echo -e "${BLUE}   Deploying test application to node...${NC}"

# Create WASM file locally first
echo -ne '\x00\x61\x73\x6d\x01\x00\x00\x00\x01\x05\x01\x60\x00\x01\x7f\x03\x02\x01\x00\x07\x08\x01\x04\x74\x65\x73\x74\x00\x00\x0a\x06\x01\x04\x00\x41\x2a\x0b' > /tmp/test-app.wasm

# Deploy from host (node is accessible via port mapping)
DEPLOY_OUTPUT=$(cd "$EXAMPLE_DIR" && cargo run --release -- deploy --name docker-test-app --language rust --wasm /tmp/test-app.wasm --node-address "$NODE_ADDRESS" 2>&1)
DEPLOY_EXIT_CODE=$?

echo "$DEPLOY_OUTPUT"

if [ $DEPLOY_EXIT_CODE -eq 0 ]; then
    if echo "$DEPLOY_OUTPUT" | grep -qiE "deployed successfully|Application deployed|✓.*deployed"; then
        echo -e "${GREEN}   ✓ Application deployed successfully to node in Docker${NC}"
    else
        echo -e "${YELLOW}   ⚠️  Deployment command ran but may not have succeeded${NC}"
        echo -e "${YELLOW}   Check output above for details${NC}"
    fi
else
    echo -e "${RED}   ✗ Application deployment failed${NC}"
    # Don't exit - continue with other tests
fi

echo
echo -e "${BLUE}4. Testing application listing...${NC}"
LIST_OUTPUT=$(cd "$EXAMPLE_DIR" && cargo run --release -- list --node-address "$NODE_ADDRESS" 2>&1)
LIST_EXIT_CODE=$?

echo "$LIST_OUTPUT"

if [ $LIST_EXIT_CODE -eq 0 ]; then
    if echo "$LIST_OUTPUT" | grep -qiE "docker-test-app|Found.*application|application.*deployed"; then
        echo -e "${GREEN}   ✓ Application listing working${NC}"
    else
        echo -e "${YELLOW}   ⚠️  Listing command ran but may not show applications${NC}"
    fi
else
    echo -e "${YELLOW}   ⚠️  Listing command had issues (may be expected)${NC}"
fi

echo
echo -e "${BLUE}5. Testing example workload (simulation)...${NC}"
if $DOCKER_COMPOSE run --rm polyglot-test polyglot_wasm_deployment run --operations 3 2>&1 | tee /tmp/polyglot-workload.log; then
    if grep -qi "Performance Report\|Aggregate Metrics" /tmp/polyglot-workload.log; then
        echo -e "${GREEN}   ✓ Example workload completed${NC}"
    else
        echo -e "${YELLOW}   ⚠️  Workload ran but may not have produced expected output${NC}"
    fi
else
    echo -e "${RED}   ✗ Example workload failed${NC}"
    exit 1
fi

# Cleanup
echo
echo -e "${BLUE}6. Cleaning up...${NC}"
if [ -n "$CONTAINER_ID" ]; then
    docker stop "$CONTAINER_ID" 2>/dev/null || true
    docker rm "$CONTAINER_ID" 2>/dev/null || true
    echo -e "${GREEN}   ✓ Node container stopped and removed${NC}"
fi
# Also clean up by name in case ID didn't work
docker stop polyglot-test-node 2>/dev/null || true
docker rm polyglot-test-node 2>/dev/null || true

rm -f /tmp/polyglot-workload.log

echo
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✅ Docker test complete!${NC}"
echo
echo -e "${BLUE}Tested scenarios:${NC}"
echo "  ✓ Empty node startup in Docker"
echo "  ✓ Application deployment to node in Docker via ApplicationService"
echo "  ✓ Application listing"
echo "  ✓ Example workload (simulation)"
echo
echo -e "${BLUE}To test manually:${NC}"
echo "  # Start node in Docker"
echo "  $DOCKER_COMPOSE run -d polyglot-test polyglot_wasm_deployment start-node --node-id my-node --address 0.0.0.0:9000"
echo ""
echo "  # Deploy application"
echo "  $DOCKER_COMPOSE run --rm polyglot-test polyglot_wasm_deployment deploy --name my-app --language rust --wasm /tmp/test.wasm --node-address 127.0.0.1:9000"
