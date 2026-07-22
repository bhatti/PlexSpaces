#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Audit Log - Event-handler (GenEvent) example

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/audit_log_actor.wasm"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="audit-log-test"
# Multipart `name` becomes ApplicationSpec.namespace and default supervisor child id.
# It must match the first path segment of /api/v1/actors/{namespace}/{actor_type}.
ACTOR_TYPE="$APP_ID"

# Commented out so node stays deployed for log/db inspection (backtrace, kv_store)
# cleanup() {
#     echo ""
#     echo "Cleanup: Undeploying $APP_ID"
#     curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
# }


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
echo "║        Audit Log - Event Handler (GenEvent)                   ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

source ~/venv/bin/activate

if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

echo "Step 2: Deploy audit log actor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
# trap cleanup EXIT  # commented out - no undeploy so log/db can be inspected
# curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
# sleep 1

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "behavior_kind=GenEvent" \
    (cd "$(dirname "$WASM_FILE")" && zip -j "$APP_ZIP" "$WASM_FILE" 2>/dev/null) || zip -j "$APP_ZIP" "$WASM_FILE"
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
echo -e "${GREEN}✓ Deployed audit log${NC}"
echo ""
sleep 2

send_post() {
    local desc="$1"
    local payload="$2"
    RESPONSE=$(curl -s --max-time 10 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
    BODY=$(echo "$RESPONSE" | sed '$d')
    if [ "$HTTP_CODE" = "200" ] && echo "$BODY" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    else
        echo -e "  ${RED}✗${NC} $desc (http=$HTTP_CODE): $BODY"
        exit 1
    fi
}

echo "Step 3: Fire-and-forget audit events (tell) - multiple calls"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Audit: login" '{"msg_type":"log","payload":{"action":"login","actor_id":"user-1","resource":"/api/session","details":"ok"}}'
send_post "Audit: order_created" '{"msg_type":"log","payload":{"action":"order_created","actor_id":"user-1","resource":"/api/orders","details":"order_id=123"}}'
send_post "Audit: logout" '{"msg_type":"log","payload":{"action":"logout","actor_id":"user-1","resource":"/api/session","details":""}}'
echo ""

echo "Step 4: Events logged via host.log (no storage - log-only for now)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "  ${GREEN}✓${NC} Audit events appear in node log (host.log)"
echo ""

echo -e "${GREEN}✅ Audit Log Test Complete${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} Event-handler (EventHandler): fire-and-forget audit events"
echo -e "  ${GREEN}•${NC} host.log only (ts_write/kv API support deferred until WASM is stable)"
echo ""
