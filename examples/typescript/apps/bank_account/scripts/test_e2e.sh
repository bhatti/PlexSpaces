#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Full E2E: start empty node (scripts/server.sh), build TS→WASM, deploy, HTTP tests. No Python.
# Pattern from examples/rust/embedded/nbody_wasm/scripts/test_e2e.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$EXAMPLE_DIR/../../../../" && pwd)"
HTTP_PORT="${HTTP_PORT:-8092}"
APP_ID="bank-test-ts"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

cleanup() {
  echo ""
  echo "Cleanup: undeploy and stop node..."
  curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
  pkill -f 'plexspaces start' 2>/dev/null || true
  sleep 1
}
trap cleanup EXIT

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Bank Account (TypeScript) – Full E2E                      ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Start empty node (repo scripts/server.sh)
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 1: Start empty PlexSpaces node"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
chmod +x "$SCRIPT_DIR"/start_server.sh "$SCRIPT_DIR"/build.sh "$SCRIPT_DIR"/deploy.sh 2>/dev/null || true
cd "$REPO_ROOT"
if [ ! -f "scripts/server.sh" ]; then
  echo -e "${RED}❌ scripts/server.sh not found (run from repo root)${NC}"
  exit 1
fi
chmod +x scripts/server.sh
if ! ./scripts/server.sh; then
  echo -e "${RED}❌ Server failed to start${NC}"
  exit 1
fi
echo -e "${GREEN}✓ Node running (HTTP $HTTP_PORT)${NC}"
echo ""

# Step 2: Build TypeScript → WASM (no Python)
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 2: Build TypeScript → WASM"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$EXAMPLE_DIR"
if ! ./scripts/build.sh; then
  echo -e "${RED}❌ Build failed${NC}"
  exit 1
fi
echo ""

# Step 3: Deploy WASM
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 3: Deploy WASM"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
./scripts/deploy.sh "$HTTP_PORT" "$APP_ID"
sleep 2
echo ""

# Step 4: HTTP operations (same as Python test.sh)
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 4: Banking operations (HTTP)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

bank_op() {
  local account="$1"
  local desc="$2"
  local payload="$3"
  RESP=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$account" \
    -H "Content-Type: application/json" \
    -d "$payload" 2>/dev/null) || true
  if echo "$RESP" | grep -q '"success":true'; then
    echo -e "  ${GREEN}✓${NC} [$account] $desc"
  else
    echo -e "  ${RED}✗${NC} [$account] $desc: $RESP"
    return 1
  fi
}

bank_op "account-alice" "Balance (0)" '{"op":"balance"}'
bank_op "account-alice" "Deposit 1000" '{"op":"deposit","amount":1000}'
bank_op "account-alice" "Withdraw 200" '{"op":"withdraw","amount":200}'
bank_op "account-alice" "Balance (800)" '{"op":"balance"}'
bank_op "account-alice" "History" '{"op":"history","count":5}'
bank_op "account-alice" "Replay" '{"op":"replay"}'

echo ""
echo -e "${GREEN}✅ TypeScript bank_account E2E passed.${NC}"
echo ""
