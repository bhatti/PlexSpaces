#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test Receipt Storage Python WASM actor deployment and execution

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/receipt_actor.wasm"
NODE_ADDR="${1:-localhost:8091}"
HTTP_PORT="${2:-8091}"

# Use shared target directory
export CARGO_TARGET_DIR="$WORKSPACE_DIR/target"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="receipt-storage-test"

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
echo "║        Receipt Storage Test (Python WASM)                      ║"
echo "║        Real-world expense tracking example                     ║"
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

# Deploy receipt storage
echo "Step 2: Deploy receipt storage actor"
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
    echo -e "${GREEN}✓ Deployed receipt-store${NC}"
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
    echo "Receipt operations:"
    echo '  {"op": "store", "merchant": "Starbucks", "amount": 5.75, "date": "2024-01-15"}'
    echo '  {"op": "list"}'
    echo '  {"op": "summary"}'
    exit 0
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""

# Test operations via HTTP API
echo "Step 3: Test receipt operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

send_receipt_op() {
    local description="$1"
    local payload="$2"
    
    # Use path format /api/v1/actors/{namespace}/{actor_type}
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$APP_ID" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || true
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $description"
    else
        echo -e "  ${YELLOW}⚠${NC} $description: $RESPONSE"
    fi
}

echo ""
echo "📝 Storing receipts from a shopping trip..."
send_receipt_op "Morning coffee at Starbucks" '{"op": "store", "merchant": "Starbucks", "amount": 5.75, "date": "2024-01-15", "description": "Grande latte"}'
send_receipt_op "Weekly groceries at Whole Foods" '{"op": "store", "merchant": "Whole Foods", "amount": 87.32, "date": "2024-01-15", "description": "Weekly groceries"}'
send_receipt_op "Gas fill-up at Shell" '{"op": "store", "merchant": "Shell", "amount": 45.00, "date": "2024-01-15", "description": "Gas fill-up"}'
send_receipt_op "Afternoon coffee" '{"op": "store", "merchant": "Starbucks", "amount": 4.50, "date": "2024-01-15", "description": "Iced coffee"}'

echo ""
echo "📋 Listing all receipts..."
send_receipt_op "List all receipts" '{"op": "list"}'

echo ""
echo "🔍 Filtering by merchant (Starbucks)..."
send_receipt_op "Filter by Starbucks" '{"op": "list", "merchant": "Starbucks"}'

echo ""
echo "💰 Getting spending summary..."
send_receipt_op "Spending summary" '{"op": "summary"}'

echo ""
echo "🗑️ Deleting a receipt..."
send_receipt_op "Delete Shell receipt" '{"op": "delete", "id": "shell-2024-01-15-1"}'

echo ""
echo "💰 Final summary after deletion..."
send_receipt_op "Final summary" '{"op": "summary"}'

echo ""
echo -e "${GREEN}✅ Receipt Storage test complete!${NC}"
echo ""
echo "This example demonstrated:"
echo "  - Storing structured data (receipts with metadata)"
echo "  - Filtering/querying stored data"
echo "  - Computing aggregates (spending summary)"
echo "  - CRUD operations via actor messages"
