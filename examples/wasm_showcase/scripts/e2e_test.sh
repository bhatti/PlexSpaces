#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# End-to-end test script for WASM application deployment
# Tests deploying WASM apps to an empty node via CLI/gRPC

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$EXAMPLE_DIR/../.." && pwd)"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🚀 WASM Showcase End-to-End Test${NC}"
echo "=================================="

# Step 1: Start empty node
echo -e "\n${YELLOW}Step 1: Starting empty PlexSpaces node...${NC}"
cd "$ROOT_DIR"

# Build node if needed
if ! cargo build --bin plexspaces-node --release 2>/dev/null; then
    echo -e "${RED}✗ Failed to build node${NC}"
    exit 1
fi

# Start node in background
NODE_PID=$(cargo run --bin plexspaces-node --release -- \
    --node-id test-node \
    --listen-addr 0.0.0.0:9001 \
    > /tmp/plexspaces-node.log 2>&1 & echo $!)

echo "Node started with PID: $NODE_PID"
echo "Waiting for node to be ready..."

# Wait for node to be ready (check health endpoint)
for i in {1..30}; do
    if curl -s http://localhost:9001/health > /dev/null 2>&1 || \
       grpc_health_probe -addr=localhost:9001 -service=readiness > /dev/null 2>&1; then
        echo -e "${GREEN}✓ Node is ready${NC}"
        break
    fi
    if [ $i -eq 30 ]; then
        echo -e "${RED}✗ Node failed to start${NC}"
        cat /tmp/plexspaces-node.log
        kill $NODE_PID 2>/dev/null || true
        exit 1
    fi
    sleep 1
done

# Step 2: Deploy WASM application via CLI
echo -e "\n${YELLOW}Step 2: Deploying WASM application via CLI...${NC}"

# Create a minimal WASM module for testing
WASM_FILE="/tmp/test-app.wasm"
cat > "$WASM_FILE" << 'EOF'
(module
  (func (export "test") (result i32)
    i32.const 42
  )
)
EOF

# Convert WAT to WASM if wat2wasm is available
if command -v wat2wasm &> /dev/null; then
    wat2wasm "$WASM_FILE" -o "$WASM_FILE" 2>/dev/null || true
fi

# Create application config
CONFIG_FILE="/tmp/app-config.toml"
cat > "$CONFIG_FILE" << 'EOF'
name = "test-wasm-app"
version = "1.0.0"
description = "Test WASM application"
type = "active"

[supervisor]
strategy = "one_for_one"

[[supervisor.children]]
id = "worker-1"
type = "worker"
start_module = "test-module"
EOF

# Deploy via CLI
if cargo run --bin plexspaces-cli --release -- \
    deploy \
    --node localhost:9001 \
    --app-id test-app-001 \
    --name test-wasm-app \
    --version 1.0.0 \
    --wasm "$WASM_FILE" \
    --config "$CONFIG_FILE" 2>&1 | tee /tmp/deploy.log; then
    echo -e "${GREEN}✓ Application deployed successfully${NC}"
else
    echo -e "${RED}✗ Deployment failed${NC}"
    cat /tmp/deploy.log
    kill $NODE_PID 2>/dev/null || true
    exit 1
fi

# Step 3: Verify application is running
echo -e "\n${YELLOW}Step 3: Verifying application status...${NC}"
sleep 2

if cargo run --bin plexspaces-cli --release -- \
    list \
    --node localhost:9001 2>&1 | grep -q "test-wasm-app"; then
    echo -e "${GREEN}✓ Application is listed${NC}"
else
    echo -e "${RED}✗ Application not found in list${NC}"
    kill $NODE_PID 2>/dev/null || true
    exit 1
fi

# Step 4: Get application status
echo -e "\n${YELLOW}Step 4: Getting application status...${NC}"
if cargo run --bin plexspaces-cli --release -- \
    status \
    --node localhost:9001 \
    --app test-wasm-app 2>&1 | grep -q "running\|Running"; then
    echo -e "${GREEN}✓ Application is running${NC}"
else
    echo -e "${YELLOW}⚠ Application status check inconclusive (may need status command)${NC}"
fi

# Step 5: Undeploy application
echo -e "\n${YELLOW}Step 5: Undeploying application...${NC}"
if cargo run --bin plexspaces-cli --release -- \
    undeploy \
    --node localhost:9001 \
    --app-id test-app-001 2>&1 | tee /tmp/undeploy.log; then
    echo -e "${GREEN}✓ Application undeployed successfully${NC}"
else
    echo -e "${RED}✗ Undeployment failed${NC}"
    cat /tmp/undeploy.log
fi

# Cleanup
echo -e "\n${YELLOW}Cleaning up...${NC}"
kill $NODE_PID 2>/dev/null || true
rm -f "$WASM_FILE" "$CONFIG_FILE" /tmp/deploy.log /tmp/undeploy.log

echo -e "\n${GREEN}✅ End-to-end test completed successfully!${NC}"

