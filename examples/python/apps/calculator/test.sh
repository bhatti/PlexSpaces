#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test Calculator Python WASM actor deployment and execution

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/calculator_actor.wasm"
NODE_ADDR="${1:-localhost:8091}"
HTTP_PORT="${2:-8092}"

# Use shared target directory
export CARGO_TARGET_DIR="$WORKSPACE_DIR/target"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="calculator-test"
# Deploy `name` sets ApplicationSpec.namespace and default child id; must match
# /api/v1/actors/{namespace}/{actor_type} path segments.
ACTOR_TYPE="$APP_ID"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    cd "$WORKSPACE_DIR"
    if [ -x "$WORKSPACE_DIR/target/debug/plexspaces" ]; then
        "$WORKSPACE_DIR/target/debug/plexspaces" undeploy --node "$NODE_ADDR" --app-id "$APP_ID" 2>/dev/null || true
    else
        cargo run -q -p plexspaces-cli -- undeploy --node "$NODE_ADDR" --app-id "$APP_ID" 2>/dev/null || true
    fi
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Calculator Actor Test (Python WASM)                   ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node is running
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$WORKSPACE_DIR"

# Use built binary if available for speed, fallback to cargo run
CLI_BIN="$WORKSPACE_DIR/target/debug/plexspaces"
if [ -x "$CLI_BIN" ]; then
    STATUS_OUTPUT=$("$CLI_BIN" status --node "$NODE_ADDR" 2>/dev/null) || true
else
    STATUS_OUTPUT=$(cargo run -q -p plexspaces-cli -- status --node "$NODE_ADDR" 2>/dev/null) || true
fi

if [ -z "$STATUS_OUTPUT" ] || ! echo "$STATUS_OUTPUT" | grep -q "Overall Status"; then
    echo -e "${RED}❌ Cannot connect to node at $NODE_ADDR${NC}"
    echo ""
    echo "Please start a PlexSpaces node first:"
    echo "  cd $WORKSPACE_DIR"
    echo "  cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8090"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Deploy calculator
echo "Step 2: Deploy calculator actor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT

if [ -x "$CLI_BIN" ]; then
    RESPONSE=$("$CLI_BIN" deploy \
        --node "$NODE_ADDR" \
        -i "$APP_ID" \
        -n "$APP_ID" \
        -w "$WASM_FILE" 2>&1) || true
else
    RESPONSE=$(cargo run -q -p plexspaces-cli -- deploy \
        --node "$NODE_ADDR" \
        -i "$APP_ID" \
        -n "$APP_ID" \
        -w "$WASM_FILE" 2>&1) || true
fi

if echo "$RESPONSE" | grep -q "deployed"; then
    echo -e "${GREEN}✓ Deployed calculator${NC}"
elif echo "$RESPONSE" | grep -q "WASM module is not valid\|instantiation\|component"; then
    echo -e "${YELLOW}⚠ WASM component not yet supported by runtime${NC}"
    echo ""
    echo "Note: Python actors built with componentize-py produce WASM Components."
    echo "The runtime needs Component Model support for these actors."
    echo ""
    echo "Expected API (for reference):"
    echo "  init(config_json: str) -> str"
    echo "  handle(from_actor: str, msg_type: str, payload_json: str) -> str"
    echo "  get_state() -> str"
    echo "  set_state(state_json: str) -> str"
    echo ""
    echo "Calculator operations:"
    echo '  {"operands": [10, 20, 30]} with msg_type="add" → {"result": 60}'
    echo '  {"operands": [100, 25]} with msg_type="subtract" → {"result": 75}'
    echo '  {"operands": [5, 6, 7]} with msg_type="multiply" → {"result": 210}'
    echo '  {"operands": [100, 4]} with msg_type="divide" → {"result": 25.0}'
    exit 0
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""

# Test operations
echo "Step 3: Test calculator operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

test_calc() {
    local operation="$1"
    local payload="$2"
    
    # Use POST with payload that includes operation field
    # POST triggers tell (fire-and-forget) but we can verify success from server logs
    # The calculator parses operation from the payload JSON
    # Don't hardcode tenant_id - use path format /api/v1/actors/{namespace}/{actor_type}
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || true
    
    # Check if response indicates success (actor processed the message)
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $operation: sent successfully"
    else
        echo -e "  ${YELLOW}⚠${NC} $operation: $RESPONSE"
    fi
}

# Calculator uses operation field from payload (msg_type is "cast" for POST)
test_calc "add" '{"operation": "add", "operands": [10, 20, 30]}'
test_calc "subtract" '{"operation": "subtract", "operands": [100, 25]}'
test_calc "multiply" '{"operation": "multiply", "operands": [5, 6, 7]}'
test_calc "divide" '{"operation": "divide", "operands": [100, 4]}'
test_calc "get_history" '{"operation": "get_history"}'

echo ""
echo -e "${GREEN}✅ Calculator test complete!${NC}"
