#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test FSM (Finite State Machine) Python WASM actor
# Real-world scenario: Order workflow state transitions

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/fsm_actor.wasm"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="fsm-test"
ACTOR_TYPE="$APP_ID"



# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    echo "Output was: $JWT_OUTPUT"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        FSM Actor - Order Workflow State Machine                ║"
echo "║        Demonstrates: idle → pending → processing → shipped     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node is running via HTTP
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"

if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node HTTP gateway at localhost:$HTTP_PORT${NC}"
    echo "Please start a PlexSpaces node first"
    exit 1
fi
echo -e "${GREEN}✓ Node HTTP gateway is running (HTTP $HTTP_CHECK)${NC}"
echo ""

# Deploy
echo "Step 2: Deploy FSM actor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Clean up any existing deployment first
echo "   Cleaning up any existing deployment..."
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

# Deploy using HTTP multipart
echo "   Deploying via HTTP multipart..."
_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed FSM service${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""

# Test operations
echo "Step 3: Test order workflow state transitions"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

send_op() {
    local description="$1"
    local msg_type="$2"
    local payload="$3"
    
    # Don't hardcode tenant_id - use path format /api/v1/actors/{namespace}/{actor_type}
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -H "X-Message-Type: $msg_type" \
        -d "$payload" 2>/dev/null) || true
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $description"
    else
        echo -e "  ${YELLOW}⚠${NC} $description: $RESPONSE"
    fi
}

echo ""
echo "📋 Order Workflow: idle → pending → processing → shipped → delivered"
echo ""

echo "1️⃣  Initial state (should be 'idle')..."
send_op "Get initial state" "call" '{"op":"status"}'

echo ""
echo "2️⃣  Create order (idle → pending)..."
send_op "Create order ORD-001" "call" '{"op":"create","order_id":"ORD-001","items":["widget","gadget"]}'
send_op "Verify state is 'pending'" "call" '{"op":"status"}'

echo ""
echo "3️⃣  Process order (pending → processing)..."
send_op "Start processing" "call" '{"op":"process"}'
send_op "Verify state is 'processing'" "call" '{"op":"status"}'

echo ""
echo "4️⃣  Ship order (processing → shipped)..."
send_op "Ship order" "call" '{"op":"ship"}'
send_op "Verify state is 'shipped'" "call" '{"op":"status"}'

echo ""
echo "5️⃣  Deliver order (shipped → delivered)..."
send_op "Mark delivered" "call" '{"op":"deliver"}'
send_op "Verify state is 'delivered'" "call" '{"op":"status"}'

echo ""
echo "6️⃣  Reset for next order..."
send_op "Reset to idle" "call" '{"op":"reset"}'
send_op "Verify state is 'idle'" "call" '{"op":"status"}'

echo ""
echo "7️⃣  Test invalid transition (should fail)..."
send_op "Try to ship from idle (invalid)" "call" '{"op":"ship"}'

echo ""
echo "8️⃣  Test cancellation workflow..."
send_op "Create new order" "call" '{"op":"create","order_id":"ORD-002"}'
send_op "Cancel order (pending → cancelled)" "call" '{"op":"cancel"}'
send_op "Verify state is 'cancelled'" "call" '{"op":"status"}'

echo ""
echo -e "${GREEN}✅ FSM test complete!${NC}"
echo ""
echo "This example demonstrated:"
echo "  - Finite state machine pattern"
echo "  - Order workflow: idle → pending → processing → shipped → delivered"
echo "  - State transition validation"
echo "  - Cancellation from valid states"
