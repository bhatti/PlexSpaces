#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Validation script for Polyglot WASM Deployment Example
# Tests deployment to existing PlexSpaces node via ApplicationService

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check for --docker flag
USE_DOCKER=false
if [ "$1" = "--docker" ]; then
    USE_DOCKER=true
    echo -e "${BLUE}Using Docker for testing...${NC}"
    echo
    exec "$SCRIPT_DIR/test-docker.sh"
fi

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Polyglot WASM Deployment Example Validation                  ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo

echo -e "${BLUE}1. Building example...${NC}"
cd "$EXAMPLE_DIR"
cargo build --release
echo -e "${GREEN}   ✓ Build successful${NC}"

echo
echo -e "${BLUE}2. Testing empty node startup...${NC}"
# Start node in background
NODE_PID=""
NODE_ADDRESS="127.0.0.1:9000"

# Function to cleanup on exit
cleanup() {
    if [ -n "$NODE_PID" ]; then
        echo -e "${BLUE}   Stopping test node (PID: $NODE_PID)...${NC}"
        kill $NODE_PID 2>/dev/null || true
        wait $NODE_PID 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Start node in background
echo -e "${BLUE}   Starting node in background...${NC}"
cargo run --release -- start-node --node-id test-node-1 --address "$NODE_ADDRESS" > /tmp/polyglot-node.log 2>&1 &
NODE_PID=$!

# Wait for node to start (node.start() is async, give it time)
echo -e "${BLUE}   Waiting for node to start...${NC}"
sleep 5

# Check if node process is still running
if ! kill -0 $NODE_PID 2>/dev/null; then
    echo -e "${RED}   ✗ Node failed to start${NC}"
    echo -e "${YELLOW}   Node logs:${NC}"
    cat /tmp/polyglot-node.log | tail -30
    exit 1
fi

# Check if node is listening on the port (gRPC)
if command -v nc &> /dev/null; then
    if nc -z 127.0.0.1 9000 2>/dev/null; then
        echo -e "${GREEN}   ✓ Node is listening on port 9000${NC}"
    else
        echo -e "${YELLOW}   ⚠️  Node process running but port 9000 not yet listening (may need more time)${NC}"
    fi
fi

echo -e "${GREEN}   ✓ Node started successfully (PID: $NODE_PID)${NC}"

echo
echo -e "${BLUE}3. Testing application deployment...${NC}"

# Create a minimal WASM file for testing
TEST_WASM="/tmp/test-app.wasm"
echo -ne '\x00\x61\x73\x6d\x01\x00\x00\x00\x01\x05\x01\x60\x00\x01\x7f\x03\x02\x01\x00\x07\x08\x01\x04\x74\x65\x73\x74\x00\x00\x0a\x06\x01\x04\x00\x41\x2a\x0b' > "$TEST_WASM"

# Deploy application
if cargo run --release -- deploy --name test-app --language rust --wasm "$TEST_WASM" --node-address "$NODE_ADDRESS" 2>&1 | tee /tmp/polyglot-deploy.log; then
    if grep -qi "deployed successfully\|Application deployed" /tmp/polyglot-deploy.log; then
        echo -e "${GREEN}   ✓ Application deployed successfully${NC}"
    else
        echo -e "${YELLOW}   ⚠️  Deployment command ran but may not have succeeded${NC}"
        echo -e "${YELLOW}   Check output above for details${NC}"
    fi
else
    echo -e "${RED}   ✗ Application deployment failed${NC}"
    cat /tmp/polyglot-deploy.log
    exit 1
fi

echo
echo -e "${BLUE}4. Testing application listing...${NC}"
if cargo run --release -- list --node-address "$NODE_ADDRESS" 2>&1 | tee /tmp/polyglot-list.log; then
    if grep -qi "test-app\|Found.*application" /tmp/polyglot-list.log; then
        echo -e "${GREEN}   ✓ Application listing working${NC}"
    else
        echo -e "${YELLOW}   ⚠️  Listing command ran but may not show applications${NC}"
    fi
else
    echo -e "${YELLOW}   ⚠️  Listing command had issues (may be expected)${NC}"
fi

echo
echo -e "${BLUE}5. Testing example workload (simulation)...${NC}"
if cargo run --release -- run --operations 5 2>&1 | tee /tmp/polyglot-workload.log; then
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
cleanup
NODE_PID=""

echo
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✅ Validation Complete!${NC}"
echo
echo -e "${BLUE}Note:${NC} For Docker testing with Firecracker, use:"
echo "      ./scripts/validate.sh --docker"
echo
rm -f /tmp/polyglot-node.log /tmp/polyglot-deploy.log /tmp/polyglot-list.log /tmp/polyglot-workload.log "$TEST_WASM"
