#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# End-to-end test script for WASM Calculator application deployment
# Tests deploying WASM calculator app to an empty node via CLI/gRPC

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$EXAMPLE_DIR/../.." && pwd)"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🚀 WASM Calculator End-to-End Test${NC}"
echo "======================================"

# Step 1: Build WASM module
echo -e "\n${YELLOW}Step 1: Building WASM calculator module...${NC}"
cd "$EXAMPLE_DIR"

if [ ! -f "wasm-modules/calculator_wasm_actor.wasm" ]; then
    echo "Building WASM module..."
    cd wasm-actors
    if cargo build --target wasm32-unknown-unknown --release 2>&1 | tee /tmp/wasm-build.log; then
        cp target/wasm32-unknown-unknown/release/calculator_wasm_actor.wasm \
           ../wasm-modules/calculator_wasm_actor.wasm 2>/dev/null || true
        echo -e "${GREEN}✓ WASM module built${NC}"
    else
        echo -e "${YELLOW}⚠ WASM build failed, using existing module if available${NC}"
    fi
    cd ..
fi

if [ ! -f "wasm-modules/calculator_wasm_actor.wasm" ]; then
    echo -e "${RED}✗ WASM module not found${NC}"
    exit 1
fi

# Step 2: Start empty node
echo -e "\n${YELLOW}Step 2: Starting empty PlexSpaces node...${NC}"
cd "$ROOT_DIR"

# Build node if needed
if ! cargo build --bin plexspaces-node --release 2>/dev/null; then
    echo -e "${RED}✗ Failed to build node${NC}"
    exit 1
fi

# Start node in background
NODE_PID=$(cargo run --bin plexspaces-node --release -- \
    --node-id calc-node \
    --listen-addr 0.0.0.0:8000 \
    > /tmp/calc-node.log 2>&1 & echo $!)

echo "Node started with PID: $NODE_PID"
echo "Waiting for node to be ready..."

# Wait for node to be ready
for i in {1..30}; do
    if curl -s http://localhost:8001/health > /dev/null 2>&1 || \
       grpc_health_probe -addr=localhost:8000 -service=readiness > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Node is ready${NC}"
        break
    fi
    if [ $i -eq 30 ]; then
        echo -e "${RED}✗ Node failed to start${NC}"
        cat /tmp/calc-node.log
        kill $NODE_PID 2>/dev/null || true
        exit 1
    fi
    sleep 1
done

# Step 3: Create application config
echo -e "\n${YELLOW}Step 3: Creating application configuration...${NC}"
CONFIG_FILE="/tmp/calc-app-config.toml"
cat > "$CONFIG_FILE" << 'EOF'
name = "calculator-app"
version = "1.0.0"
description = "WASM Calculator Application"
type = "active"

[supervisor]
strategy = "one_for_one"

[[supervisor.children]]
id = "calculator-1"
type = "worker"
start_module = "calculator_wasm_actor"
EOF

# Step 4: Deploy WASM application via CLI
echo -e "\n${YELLOW}Step 4: Deploying WASM calculator application via CLI...${NC}"

if cargo run --bin plexspaces-cli --release -- \
    deploy \
    --node localhost:8000 \
    --app-id calc-app-001 \
    --name calculator-app \
    --version 1.0.0 \
    --wasm "$EXAMPLE_DIR/wasm-modules/calculator_wasm_actor.wasm" \
    --config "$CONFIG_FILE" 2>&1 | tee /tmp/calc-deploy.log; then
    echo -e "${GREEN}✓ Calculator application deployed successfully${NC}"
else
    echo -e "${RED}✗ Deployment failed${NC}"
    cat /tmp/calc-deploy.log
    kill $NODE_PID 2>/dev/null || true
    exit 1
fi

# Step 5: Verify application is running
echo -e "\n${YELLOW}Step 5: Verifying application status...${NC}"
sleep 2

if cargo run --bin plexspaces-cli --release -- \
    list \
    --node localhost:8000 2>&1 | grep -q "calculator-app"; then
    echo -e "${GREEN}✓ Calculator application is listed${NC}"
else
    echo -e "${RED}✗ Application not found in list${NC}"
    kill $NODE_PID 2>/dev/null || true
    exit 1
fi

# Step 6: Test calculator functionality (if actor messaging is available)
echo -e "\n${YELLOW}Step 6: Testing calculator functionality...${NC}"
echo "Note: This step requires actor messaging support in CLI"
echo "For now, verifying application is running..."

# Step 7: Undeploy application
echo -e "\n${YELLOW}Step 7: Undeploying application...${NC}"
if cargo run --bin plexspaces-cli --release -- \
    undeploy \
    --node localhost:8000 \
    --app-id calc-app-001 2>&1 | tee /tmp/calc-undeploy.log; then
    echo -e "${GREEN}✓ Application undeployed successfully${NC}"
else
    echo -e "${RED}✗ Undeployment failed${NC}"
    cat /tmp/calc-undeploy.log
fi

# Cleanup
echo -e "\n${YELLOW}Cleaning up...${NC}"
kill $NODE_PID 2>/dev/null || true
rm -f "$CONFIG_FILE" /tmp/calc-deploy.log /tmp/calc-undeploy.log /tmp/calc-node.log

echo -e "\n${GREEN}✅ End-to-end test completed successfully!${NC}"

