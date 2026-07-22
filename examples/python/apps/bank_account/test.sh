#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Bank Account - Basic Operations (no server restart)
# For durability testing with restart, use test-durability.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/account_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# gRPC and HTTP share a single port (default: 8091)
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="bank-test"



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

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
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

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
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
    
    # Path /api/v1/actors/{namespace}/{actor_type}: namespace must match ApplicationSpec
    # (multipart deploy `name` defaults TOML namespace when [namespace] is omitted).
    # Don't hardcode tenant_id - tenant from header/JWT when auth is enabled.
    # This gets tenant from header/JWT (empty when auth disabled)
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$account" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
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
