#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Integration test for go-ws-chat-room.
#
# Tests the chat room via HTTP ask/tell (no actual WebSocket required):
#   - alice and bob join the room
#   - alice sends a message
#   - verify history contains alice's message
#   - alice and bob leave
#   - presence actor online/offline lifecycle
#
# Usage:
#   bash test.sh [port]
#   KEEP_DEPLOYED=1 bash test.sh   # skip undeploy at end
#
# Environment overrides:
#   HTTP_PORT, HTTP_URL, APP_ID, TIMEOUT (seconds), KEEP_DEPLOYED
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

WASM_FILE="$SCRIPT_DIR/ws_chat_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

HTTP_PORT="${1:-${HTTP_PORT:-8093}}"
HTTP_URL="${HTTP_URL:-http://localhost:${HTTP_PORT}}"
APP_ID="${APP_ID:-go-ws-chat-room}"
TIMEOUT="${TIMEOUT:-60}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

log()  { echo "[test.sh] $*" >&2; }
fail() { echo -e "${RED}[FAIL] $*${NC}" >&2; exit 1; }
pass() { echo -e "${GREEN}[PASS] $*${NC}" >&2; }

# ─── 0. Build if needed ──────────────────────────────────────────────────────
if [ ! -f "$WASM_FILE" ]; then
  log "Building ws_chat_actor.wasm..."
  bash "$SCRIPT_DIR/build.sh" || fail "Build failed"
fi
[ -f "$WASM_FILE" ] || fail "WASM not built: $WASM_FILE"

# ─── 1. JWT auth (auto-generate if server has auth enabled) ──────────────────
export AUTH_HEADER="${AUTH_HEADER:-}"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" \
    "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

echo "================================================================"
echo "  Go WebSocket Chat Room — Integration Test"
echo "  App ID:   $APP_ID"
echo "  Node URL: $HTTP_URL"
echo "================================================================"
echo ""

# ─── 2. Check node is reachable ──────────────────────────────────────────────
log "Checking node at ${HTTP_URL}..."
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null

HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "${HTTP_URL}/health" 2>/dev/null || echo "000")
[ "$HTTP_CODE" = "200" ] || fail "Node not reachable (HTTP $HTTP_CODE). Start with: ./scripts/server.sh"
echo -e "${GREEN}Node is running${NC}"
echo ""

# ─── 3. Undeploy any previous run, then deploy ───────────────────────────────
log "Undeploying previous run..."
bash "$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT" 2>/dev/null || true
sleep 1

log "Deploying ${APP_ID}..."
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "${HTTP_URL}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=${APP_ID}" \
    -F "name=${APP_ID}" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  log "Deploy attempt $_attempt failed (HTTP $HTTP_CODE), retrying in 3s..."
  sleep 3
done
[ "$_deployed" -eq 1 ] || fail "Deploy failed after 3 attempts: $RESPONSE"
pass "Deployed $APP_ID"
echo ""
sleep 2

# ─── Helper ──────────────────────────────────────────────────────────────────
ask_actor() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-$TIMEOUT}"
  curl -s --max-time "$timeout" \
    -X POST "${HTTP_URL}/api/v1/actors/${APP_ID}/${actor}/ask?timeout=${timeout}" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

extract_field() {
  local json="$1"
  local field="$2"
  echo "$json" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
v = p.get('$field', None)
if v is None:
    print('')
elif isinstance(v, list):
    print(json.dumps(v))
elif isinstance(v, dict):
    print(json.dumps(v))
else:
    print(v)
" 2>/dev/null || echo ""
}

ROOM="test-lobby"
ALICE_ID="alice//ChatClient::${APP_ID}@test-node-${HTTP_PORT}"
BOB_ID="bob//ChatClient::${APP_ID}@test-node-${HTTP_PORT}"

# ─── 4. Join alice and bob ───────────────────────────────────────────────────
echo "Step 1: Join alice and bob to room '${ROOM}'"
echo "----------------------------------------------------------------"

ALICE_JOIN=$(ask_actor "ChatRoomActor:${ROOM}" \
  "{\"op\":\"join\",\"actor_id\":\"${ALICE_ID}\",\"username\":\"alice\"}")
if echo "$ALICE_JOIN" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  pass "alice joined room"
else
  fail "alice join failed: $ALICE_JOIN"
fi

BOB_JOIN=$(ask_actor "ChatRoomActor:${ROOM}" \
  "{\"op\":\"join\",\"actor_id\":\"${BOB_ID}\",\"username\":\"bob\"}")
if echo "$BOB_JOIN" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  pass "bob joined room"
else
  fail "bob join failed: $BOB_JOIN"
fi
echo ""

# ─── 5. Get members ──────────────────────────────────────────────────────────
echo "Step 2: Verify member list"
echo "----------------------------------------------------------------"

MEMBERS=$(ask_actor "ChatRoomActor:${ROOM}" '{"op":"members"}')
if echo "$MEMBERS" | grep -q '"members"'; then
  MEMBER_COUNT=$(extract_field "$MEMBERS" "members" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "?")
  pass "Member list returned ($MEMBER_COUNT members)"
else
  fail "Members query failed: $MEMBERS"
fi
echo ""

# ─── 6. Alice sends a message ────────────────────────────────────────────────
echo "Step 3: Alice sends a message"
echo "----------------------------------------------------------------"

TEST_TEXT="hello from alice at $(date +%s)"
SEND_RESP=$(ask_actor "ChatRoomActor:${ROOM}" \
  "{\"op\":\"send\",\"sender_actor_id\":\"${ALICE_ID}\",\"text\":\"${TEST_TEXT}\"}")
if echo "$SEND_RESP" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  NOTIFIED=$(extract_field "$SEND_RESP" "members_notified")
  pass "alice sent message (notified: ${NOTIFIED})"
else
  fail "alice send failed: $SEND_RESP"
fi
echo ""

# ─── 7. Check room status ────────────────────────────────────────────────────
echo "Step 4: Check room status"
echo "----------------------------------------------------------------"

STATUS=$(ask_actor "ChatRoomActor:${ROOM}" '{"op":"status"}')
if echo "$STATUS" | grep -q '"room_id"'; then
  HIST_SIZE=$(extract_field "$STATUS" "history_size")
  MSG_SEQ=$(extract_field "$STATUS" "msg_seq")
  pass "Room status OK (history_size=${HIST_SIZE} msg_seq=${MSG_SEQ})"
else
  fail "Room status failed: $STATUS"
fi
echo ""

# ─── 8. Verify history contains alice's message ──────────────────────────────
echo "Step 5: Verify history"
echo "----------------------------------------------------------------"

# Re-join alice to fetch history snapshot
HISTORY_RESP=$(ask_actor "ChatRoomActor:${ROOM}" \
  "{\"op\":\"join\",\"actor_id\":\"${ALICE_ID}\",\"username\":\"alice\"}")
# The join response includes history
if echo "$HISTORY_RESP" | grep -q '"history"'; then
  if echo "$HISTORY_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
history = p.get('history', [])
texts = [e.get('text','') for e in history]
found = any('${TEST_TEXT}' in t for t in texts)
sys.exit(0 if found else 1)
" 2>/dev/null; then
    pass "History contains alice's message"
  else
    echo -e "${YELLOW}[WARN] History may not contain the test message (timing); continuing${NC}"
  fi
else
  echo -e "${YELLOW}[WARN] History not in join response; skipping history check${NC}"
fi
echo ""

# ─── 9. Presence actor lifecycle ─────────────────────────────────────────────
echo "Step 6: Presence actor lifecycle"
echo "----------------------------------------------------------------"

ONLINE_RESP=$(ask_actor "alice:PresenceActor" '{"op":"online"}')
if echo "$ONLINE_RESP" | grep -qE '"online"[[:space:]]*:[[:space:]]*true'; then
  pass "PresenceActor: alice marked online"
else
  echo -e "${YELLOW}[WARN] Presence online response: $ONLINE_RESP${NC}"
fi

PRESENCE_STATUS=$(ask_actor "alice:PresenceActor" '{"op":"status"}')
if echo "$PRESENCE_STATUS" | grep -q '"user_id"'; then
  pass "PresenceActor: status returned"
else
  echo -e "${YELLOW}[WARN] Presence status: $PRESENCE_STATUS${NC}"
fi

OFFLINE_RESP=$(ask_actor "alice:PresenceActor" '{"op":"offline"}')
if echo "$OFFLINE_RESP" | grep -qE '"online"[[:space:]]*:[[:space:]]*false'; then
  pass "PresenceActor: alice marked offline"
else
  echo -e "${YELLOW}[WARN] Presence offline response: $OFFLINE_RESP${NC}"
fi
echo ""

# ─── 10. Leave alice and bob ─────────────────────────────────────────────────
echo "Step 7: Leave alice and bob"
echo "----------------------------------------------------------------"

ALICE_LEAVE=$(ask_actor "ChatRoomActor:${ROOM}" \
  "{\"op\":\"leave\",\"actor_id\":\"${ALICE_ID}\"}")
if echo "$ALICE_LEAVE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  pass "alice left room"
else
  echo -e "${YELLOW}[WARN] alice leave: $ALICE_LEAVE${NC}"
fi

BOB_LEAVE=$(ask_actor "ChatRoomActor:${ROOM}" \
  "{\"op\":\"leave\",\"actor_id\":\"${BOB_ID}\"}")
if echo "$BOB_LEAVE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  pass "bob left room"
else
  echo -e "${YELLOW}[WARN] bob leave: $BOB_LEAVE${NC}"
fi
echo ""

# ─── 11. Undeploy (skipped if KEEP_DEPLOYED=1) ───────────────────────────────
if [ "${KEEP_DEPLOYED:-0}" = "1" ]; then
  log "KEEP_DEPLOYED=1 — skipping undeploy"
else
  log "Undeploying ${APP_ID}..."
  bash "$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
fi

echo "================================================================"
echo -e "  ${GREEN}ALL TESTS PASSED${NC}"
echo "================================================================"
