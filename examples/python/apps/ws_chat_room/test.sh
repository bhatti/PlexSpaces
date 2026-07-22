#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Integration test for the Python ws_chat_room example.
#
# Tests via HTTP ask/tell API:
#   1. Deploy the WASM application.
#   2. Alice and Bob join the same chat room.
#   3. Alice sends a message; verify the send returns success.
#   4. Fetch room members and status.
#   5. Alice and Bob leave.
#   6. Undeploy (unless KEEP_DEPLOYED=1).
#
# Usage:
#   bash test.sh [port]
#   KEEP_DEPLOYED=1 bash test.sh   # skip undeploy
#
# Environment overrides:
#   HTTP_PORT, HTTP_HOST, KEEP_DEPLOYED

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/ws_chat_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-${HTTP_PORT:-8094}}"
HTTP_HOST="${HTTP_HOST:-127.0.0.1}"

APP_ID="py-ws-chat-room"
ROOM_ID="test-room-$$"
ALICE_ID="alice//ChatClient::${APP_ID}@test-node"
BOB_ID="bob//ChatClient::${APP_ID}@test-node"
CHAT_ACTOR="ChatRoomActor:${ROOM_ID}"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# ─── JWT auth ──────────────────────────────────────────────────────────────────
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

# ─── Helpers ───────────────────────────────────────────────────────────────────
ask_actor() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://$HTTP_HOST:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

tell_actor() {
  local actor="$1"
  local payload="$2"
  curl -s --max-time 10 -X POST \
    "http://$HTTP_HOST:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/tell" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

extract_payload_json() {
  local raw="$1"
  RAW_RESPONSE="$raw" python3 - <<'PY'
import json, os
raw = os.environ["RAW_RESPONSE"]
data = json.loads(raw)
payload = data.get("payload", data)
if isinstance(payload, str):
    try:
        payload = json.loads(payload)
    except Exception:
        payload = {"value": payload}
print(json.dumps(payload))
PY
}

assert_actor_ok() {
  local step="$1"
  local raw="$2"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" python3 - <<'PY'
import json, os, sys
d = json.loads(os.environ["ASSERT_JSON"])
payload = d.get("payload", d)
if d.get("success") is False:
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: request failed", file=sys.stderr)
    sys.exit(1)
if isinstance(payload, dict) and payload.get("error"):
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: {payload['error']}", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step${NC}"
    echo "  Raw: $(echo "$raw" | head -c 500)"
    exit 1
  fi
}

assert_field() {
  local step="$1"
  local raw="$2"
  local field="$3"
  local expected="$4"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" ASSERT_FIELD="$field" ASSERT_EXPECTED="$expected" python3 - <<'PY'
import json, os, sys
d = json.loads(os.environ["ASSERT_JSON"])
payload = d.get("payload", d)
if isinstance(payload, str):
    try:
        payload = json.loads(payload)
    except Exception:
        pass
field = os.environ["ASSERT_FIELD"]
expected = os.environ["ASSERT_EXPECTED"]
value = payload
for part in field.split("."):
    value = value.get(part) if isinstance(value, dict) else None
actual = str(value)
if actual != expected:
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: {field}={actual!r}, want {expected!r}", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Field assertion failed: $step${NC}"
    echo "  Raw: $(echo "$raw" | head -c 500)"
    exit 1
  fi
}

# ─── Step 0: Build ─────────────────────────────────────────────────────────────
echo "Step 0: Ensure WASM artifact exists"
if [ ! -f "$WASM_FILE" ]; then
  "$SCRIPT_DIR/build.sh"
else
  echo "  Using existing $WASM_FILE"
fi
echo ""

# ─── Step 1: Check node reachable ──────────────────────────────────────────────
echo "Step 1: Check node at http://$HTTP_HOST:$HTTP_PORT"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://$HTTP_HOST:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start a PlexSpaces node on port $HTTP_PORT and re-run this test${NC}"
  exit 1
fi
echo "  Node reachable (HTTP $HTTP_CHECK)"
echo ""

# ─── Step 2: Deploy ────────────────────────────────────────────────────────────
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null

echo "Step 2: Deploy $APP_ID"
curl -s -X DELETE "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/$APP_ID" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} >/dev/null 2>&1 || true
sleep 1

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST \
    "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=py-ws-chat-room" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed (HTTP $HTTP_CODE), retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo "  Deployed OK"
sleep 1
echo ""

# ─── Step 3: Alice joins ───────────────────────────────────────────────────────
echo "Step 3: Alice joins the chat room"
R="$(ask_actor "$CHAT_ACTOR" "{\"actor_id\":\"$ALICE_ID\",\"username\":\"alice\"}" 20)"
assert_actor_ok "alice join" "$R"
echo "  alice join OK: $(echo "$R" | head -c 120)"
echo ""

# ─── Step 4: Bob joins ────────────────────────────────────────────────────────
echo "Step 4: Bob joins the chat room"
R="$(ask_actor "$CHAT_ACTOR" "{\"actor_id\":\"$BOB_ID\",\"username\":\"bob\"}" 20)"
assert_actor_ok "bob join" "$R"
echo "  bob join OK: $(echo "$R" | head -c 120)"
echo ""

# ─── Step 5: Alice sends a message ────────────────────────────────────────────
echo "Step 5: Alice sends a message"
TEST_TEXT="hello from alice at $(date +%s)"
R="$(ask_actor "$CHAT_ACTOR" \
  "{\"op\":\"send\",\"sender_actor_id\":\"$ALICE_ID\",\"text\":\"$TEST_TEXT\"}" 20)"
assert_actor_ok "alice send" "$R"
echo "  send OK: $(echo "$R" | head -c 120)"
echo ""

# ─── Step 6: Check room members ───────────────────────────────────────────────
echo "Step 6: Check room members"
R="$(ask_actor "$CHAT_ACTOR" '{"op":"members"}' 20)"
assert_actor_ok "members" "$R"
echo "  members OK: $(echo "$R" | head -c 200)"
echo ""

# ─── Step 7: Check room status ────────────────────────────────────────────────
echo "Step 7: Check room status"
R="$(ask_actor "$CHAT_ACTOR" '{"op":"status"}' 20)"
assert_actor_ok "status" "$R"
echo "  status OK: $(echo "$R" | head -c 200)"
echo ""

# ─── Step 8: Alice leaves ─────────────────────────────────────────────────────
echo "Step 8: Alice leaves"
R="$(ask_actor "$CHAT_ACTOR" "{\"actor_id\":\"$ALICE_ID\"}" 20)"
assert_actor_ok "alice leave" "$R"
echo "  alice leave OK"
echo ""

# ─── Step 9: Bob leaves ───────────────────────────────────────────────────────
echo "Step 9: Bob leaves"
R="$(ask_actor "$CHAT_ACTOR" "{\"actor_id\":\"$BOB_ID\"}" 20)"
assert_actor_ok "bob leave" "$R"
echo "  bob leave OK"
echo ""

# ─── Step 10: Undeploy ────────────────────────────────────────────────────────
if [ "${KEEP_DEPLOYED:-0}" = "1" ]; then
  echo "KEEP_DEPLOYED=1 — skipping undeploy"
else
  echo "Step 10: Undeploy $APP_ID"
  curl -s -X DELETE "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/$APP_ID" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} >/dev/null 2>&1 || true
  echo "  Undeployed"
fi

echo ""
echo -e "${GREEN}ALL TESTS PASSED${NC}"
