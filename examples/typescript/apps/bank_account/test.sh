#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Bank Account (TypeScript WASM): build, deploy, HTTP /ask flow.
# Expects a node already listening (from repo root: ./scripts/server.sh).
#
# Usage:
#   ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/account_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="bank-test-ts"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'



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

echo "================================================================"
echo "  Bank Account (TypeScript WASM)"
echo "================================================================"

echo "Step 1: Build WASM"
"$SCRIPT_DIR/scripts/build.sh"
echo ""

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot reach node on port $HTTP_PORT. Start it from repo root: ./scripts/server.sh${NC}"
  exit 1
fi

echo "Step 2: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1
# `name` becomes the default ApplicationSpec.namespace when TOML omits [namespace]; keep it equal to application_id for /actors/{namespace}/ routing.
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
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
echo -e "  ${GREEN}Deployed $APP_ID${NC}"
sleep 2

bank_ask() {
  local account="$1"
  local payload="$2"
  curl -s --max-time 35 -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$account/ask?timeout=30" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"success":false}'
}

bank_op() {
  local account="$1"
  local desc="$2"
  local payload="$3"
  local RESP
  RESP=$(bank_ask "$account" "$payload")
  if echo "$RESP" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "  ${GREEN}✓${NC} [$account] $desc"
  else
    echo -e "  ${RED}✗${NC} [$account] $desc: $RESP"
    return 1
  fi
}

echo "Step 3: Banking operations (HTTP /ask)"
bank_op "account-alice" "Balance (0)" '{"op":"balance"}'
bank_op "account-alice" "Deposit 1000" '{"op":"deposit","amount":1000}'
bank_op "account-alice" "Withdraw 200" '{"op":"withdraw","amount":200}'
bank_op "account-alice" "Balance (800)" '{"op":"balance"}'
bank_op "account-alice" "History" '{"op":"history","count":5}'
bank_op "account-alice" "Replay" '{"op":"replay"}'

echo ""
echo "================================================================"
echo -e "  ${GREEN}Bank Account (TypeScript) example test complete${NC}"
echo "================================================================"
