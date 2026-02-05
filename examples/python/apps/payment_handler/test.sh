#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test Payment Handler - GenServer Microservice

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/payment_handler_actor.wasm"
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="payment-handler-test"
ACTOR_NAME="PaymentHandler"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Payment Handler - GenServer Microservice               ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "${YELLOW}💡 Real-world use case: Payment processing with idempotency and locking${NC}"
echo ""

source ~/venv/bin/activate

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
echo "Step 2: Deploy payment handler actor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$ACTOR_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed payment handler${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function
send_post() {
    local desc="$1"
    local payload="$2"
    
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

echo "Step 3: Process payments (GenServer call)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Process payment #1 (\$10.00)" '{"msg_type":"process_payment","payload":{"payment_id":"pay-001","amount":1000,"currency":"USD","customer_id":"cust-1"}}'
send_post "Process payment #2 (\$25.50)" '{"msg_type":"process_payment","payload":{"payment_id":"pay-002","amount":2550,"currency":"USD","customer_id":"cust-2"}}'
echo ""

echo "Step 4: Test idempotency (duplicate payment)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Duplicate payment #1 (should return cached)" '{"msg_type":"process_payment","payload":{"payment_id":"pay-001","amount":1000,"currency":"USD","customer_id":"cust-1"}}'
echo ""

echo "Step 5: Process refund (with distributed lock)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Refund \$5.00 on tx-1" '{"msg_type":"refund","payload":{"refund_id":"ref-001","original_tx_id":"tx-1","amount":500,"reason":"partial refund"}}'
echo ""

echo "Step 6: Get balance"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Get balance" '{"msg_type":"balance","payload":{}}'
echo ""

echo "Step 7: List transactions"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "List all transactions" '{"msg_type":"list_transactions","payload":{"limit":10}}'
echo ""

echo -e "${GREEN}✅ Payment Handler Test Complete${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} GenServer pattern (request-reply)"
echo -e "  ${GREEN}•${NC} Idempotency via kv_get/kv_put"
echo -e "  ${GREEN}•${NC} Distributed locking for refunds"
echo -e "  ${GREEN}•${NC} Durable transaction state"
echo ""
echo -e "${YELLOW}💡 Deploy multiple instances behind ElasticPool for production${NC}"
echo ""
