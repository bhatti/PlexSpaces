#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${BLUE}=== WASM Calculator Integration Tests ===${NC}"
echo ""

cd "$PROJECT_DIR"

# Cleanup any previous runs
echo -e "${BLUE}Cleaning up previous runs...${NC}"
./scripts/cleanup.sh
echo ""

# Start multi-node cluster
echo -e "${BLUE}Starting multi-node cluster...${NC}"
./scripts/multi_node_start.sh
echo ""

# Wait for cluster to stabilize
echo -e "${BLUE}Waiting for cluster to stabilize...${NC}"
sleep 5

# Run integration tests
echo -e "${BLUE}Running integration tests...${NC}"
echo ""

TEST_FAILED=false

# Test 1: Calculator operations
echo -e "${YELLOW}Test 1: Basic calculator operations${NC}"
if cargo test --test calculator_integration -- --nocapture 2>&1 | tee /tmp/wasm-calc-test1.log; then
    echo -e "${GREEN}  ✓ Calculator operations test passed${NC}"
else
    echo -e "${RED}  ✗ Calculator operations test failed${NC}"
    TEST_FAILED=true
fi
echo ""

# Test 2: WASM module loading (TODO: implement when WASM runtime is integrated)
echo -e "${YELLOW}Test 2: WASM module loading${NC}"
echo -e "${YELLOW}  ⊘ WASM module loading test (TODO - pending WASM integration)${NC}"
echo ""

# Test 3: Multi-node coordination (TODO: implement distributed tests)
echo -e "${YELLOW}Test 3: Multi-node coordination${NC}"
echo -e "${YELLOW}  ⊘ Multi-node coordination test (TODO - pending distributed setup)${NC}"
echo ""

# Stop cluster
echo -e "${BLUE}Stopping cluster...${NC}"
./scripts/multi_node_stop.sh
echo ""

# Report results
if [ "$TEST_FAILED" = true ]; then
    echo -e "${RED}❌ Some tests failed${NC}"
    echo "Check logs:"
    echo "  - Node logs: logs/node*.log"
    echo "  - Test output: /tmp/wasm-calc-test*.log"
    exit 1
else
    echo -e "${GREEN}✅ All integration tests passed!${NC}"
    exit 0
fi
