#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test Bank Account - Basic Operations (no server restart)
# For durability testing with restart, use test-durability.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/account_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8092 (gRPC 8091 + 1)
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="bank-test"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Bank Account - Basic Operations Test                    ║"
echo "║        For durability test with restart: ./test-durability.sh  ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"

if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Deploy
echo "Step 2: Deploy 3 bank accounts via ApplicationSpec"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT

curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=account" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed 3 bank accounts${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function
bank_op() {
    local account="$1"
    local desc="$2"
    local payload="$3"
    
    # Use the application ID as the namespace (bank-test)
    # Don't hardcode tenant_id - use path format /api/v1/actors/{namespace}/{actor_type}
    # This gets tenant from header/JWT (empty when auth disabled)
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$account" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || true
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} [$account] $desc"
    else
        echo -e "  ${YELLOW}⚠${NC} [$account] $desc: $RESPONSE"
    fi
}

echo "Step 3: Banking operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

echo ""
echo "💰 Initial balances..."
bank_op "account-alice" "Check balance (expect 0)" '{"op":"balance"}'
bank_op "account-bob" "Check balance (expect 0)" '{"op":"balance"}'

echo ""
echo "💵 Deposits..."
bank_op "account-alice" "Deposit \$1000" '{"op":"deposit","amount":1000}'
bank_op "account-bob" "Deposit \$500" '{"op":"deposit","amount":500}'
bank_op "account-charlie" "Deposit \$250" '{"op":"deposit","amount":250}'

echo ""
echo "💸 Withdrawals..."
bank_op "account-alice" "Withdraw \$200" '{"op":"withdraw","amount":200}'
bank_op "account-bob" "Withdraw \$100" '{"op":"withdraw","amount":100}'

echo ""
echo "📊 Final balances..."
bank_op "account-alice" "Balance (expect \$800)" '{"op":"balance"}'
bank_op "account-bob" "Balance (expect \$400)" '{"op":"balance"}'
bank_op "account-charlie" "Balance (expect \$250)" '{"op":"balance"}'

echo ""
echo "📜 Transaction history..."
bank_op "account-alice" "Get history" '{"op":"history","count":5}'

echo ""
echo "🔄 Replay (rebuild state from log)..."
bank_op "account-alice" "Replay transactions" '{"op":"replay"}'

echo ""
echo "❌ Test insufficient funds..."
bank_op "account-charlie" "Withdraw \$500 (only has \$250)" '{"op":"withdraw","amount":500}'

echo ""
echo -e "${GREEN}✅ Basic operations test complete!${NC}"
echo ""
echo "To test durability with server restart, run: ./test-durability.sh"
