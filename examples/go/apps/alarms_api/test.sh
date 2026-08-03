#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Alarms API — Go WASM
#
# Verifies the Cloudflare DO alarm() pattern:
#   1. Enqueue 3 items (alarm set on first write)
#   2. Check status shows alarm is pending
#   3. Wait 12 seconds for alarm to fire
#   4. Verify queue was processed (count → 0)
#
# Usage: ./test.sh [port]
# Prerequisites: Start PlexSpaces node first.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/alarm_queue.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-${HTTP_PORT:-8091}}"
HTTP_HOST="${HTTP_HOST:-127.0.0.1}"

APP_ID="go-alarms-api"
QUEUE_ACTOR="queue-1:RequestQueueActor"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

# ── JWT auth ──────────────────────────────────────────────────────────────────
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

# ── Helpers ───────────────────────────────────────────────────────────────────
send_op() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://$HTTP_HOST:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

extract() {
  local raw="$1"
  local key="$2"
  RAW="$raw" KEY="$key" python3 -c "
import json, os
d = json.loads(os.environ['RAW'])
p = d.get('payload', d)
print(p.get(os.environ['KEY'], ''))
" 2>/dev/null || echo ""
}

echo "================================================================"
echo "  Alarms API — Go WASM"
echo "  Cloudflare DO alarm() pattern: RequestQueue batched processing"
echo "================================================================"
echo "  HTTP: $HTTP_HOST:$HTTP_PORT  App: $APP_ID"
echo ""

# ── Build if needed ───────────────────────────────────────────────────────────
if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM actor..."
  chmod +x "$SCRIPT_DIR/build.sh"
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
  echo ""
fi

# ── Step 0: Check node ────────────────────────────────────────────────────────
echo "Step 0: Check node"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://$HTTP_HOST:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node at $HTTP_HOST:$HTTP_PORT${NC}"
  exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""

# ── Step 1: Deploy ────────────────────────────────────────────────────────────
echo "Step 1: Deploy application"
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
trap 'rm -f "$APP_ZIP"' EXIT
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null

# Undeploy any previous instance
curl -s -X DELETE \
  "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/$APP_ID" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} >/dev/null 2>&1 || true
sleep 1

DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST \
  "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/deploy" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -F "application_id=$APP_ID" \
  -F "name=go-alarms-api" \
  -F "version=1.0.0" \
  -F "app_file=@$APP_ZIP" 2>&1) || true
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')

if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${GREEN}Deployed $APP_ID${NC}"
else
  echo -e "${RED}Deploy failed (HTTP $HTTP_CODE): $RESPONSE${NC}"
  exit 1
fi
sleep 2
echo ""

# ── Step 2: Reset queue ───────────────────────────────────────────────────────
echo "Step 2: Reset queue"
send_op "$QUEUE_ACTOR" '{"op":"reset"}' >/dev/null
echo -e "${GREEN}Queue reset${NC}"
echo ""

# ── Step 3: Enqueue 3 items ───────────────────────────────────────────────────
echo "Step 3: Enqueue 3 items (alarm set on first write)"
R1=$(send_op "$QUEUE_ACTOR" '{"op":"enqueue","item":{"id":1,"data":"foo"}}')
R2=$(send_op "$QUEUE_ACTOR" '{"op":"enqueue","item":{"id":2,"data":"bar"}}')
R3=$(send_op "$QUEUE_ACTOR" '{"op":"enqueue","item":{"id":3,"data":"baz"}}')
echo "  item 1: $(extract "$R1" queued) queued"
echo "  item 2: $(extract "$R2" queued) queued"
echo "  item 3: $(extract "$R3" queued) queued"
ALARM_WAS_SET=$(extract "$R1" alarm_set)
if [ "$ALARM_WAS_SET" = "True" ] || [ "$ALARM_WAS_SET" = "true" ]; then
  echo -e "  ${GREEN}Alarm set on first enqueue${NC}"
else
  echo -e "  ${YELLOW}alarm_set flag: $ALARM_WAS_SET${NC}"
fi
echo ""

# ── Step 4: Check status ──────────────────────────────────────────────────────
echo "Step 4: Check status (alarm should be pending)"
STATUS=$(send_op "$QUEUE_ACTOR" '{"op":"status"}')
COUNT=$(extract "$STATUS" count)
ALARM_SET=$(extract "$STATUS" alarm_set)
ALARM_AT=$(extract "$STATUS" alarm_at)
echo "  count=$COUNT  alarm_set=$ALARM_SET  alarm_at=$ALARM_AT"
if [ "$ALARM_SET" = "True" ] || [ "$ALARM_SET" = "true" ]; then
  echo -e "  ${GREEN}PASS: alarm is set${NC}"
else
  echo -e "  ${RED}FAIL: expected alarm_set=true${NC}"; exit 1
fi
echo ""

# ── Step 5: Wait for alarm ────────────────────────────────────────────────────
echo "Step 5: Waiting 12s for alarm to fire..."
sleep 12
echo ""

# ── Step 6: Verify queue was processed ───────────────────────────────────────
echo "Step 6: Verify queue was processed (count should be 0)"
STATUS2=$(send_op "$QUEUE_ACTOR" '{"op":"status"}')
COUNT2=$(extract "$STATUS2" count)
TOTAL=$(extract "$STATUS2" total_processed)
FIRES=$(extract "$STATUS2" total_alarm_fires)
echo "  count=$COUNT2  total_processed=$TOTAL  total_alarm_fires=$FIRES"
if [ "$COUNT2" = "0" ]; then
  echo -e "  ${GREEN}PASS: Queue processed (count=0, processed=$TOTAL, fires=$FIRES)${NC}"
else
  echo -e "  ${RED}FAIL: expected count=0, got $COUNT2${NC}"; exit 1
fi
echo ""

echo "================================================================"
echo -e "  ${GREEN}Alarms API (Go) Test Complete${NC}"
echo "================================================================"
