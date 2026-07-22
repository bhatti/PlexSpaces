#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/chat_room_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
HTTP_HOST="${HTTP_HOST:-127.0.0.1}"

APP_ID="chat-room-large-scale"
GUILD_ACTOR="GuildActor:guild-acme"
ALICE_SESSION="SessionActor:alice-mobile"
BOB_SESSION="SessionActor:bob-desktop"
CHANNEL_ACTOR="ChannelActor:guild-acme__general"
PRESENCE_ACTOR="PresenceActor:alice"
FSM_ACTOR="ConnectionFSM:alice-mobile"
WORKFLOW_ACTOR="ModerationWorkflow:report-1"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
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

wait_for_field() {
  local actor="$1"
  local payload="$2"
  local field="$3"
  local expected="$4"
  local attempts="${5:-25}"
  local sleep_s="${6:-0.2}"
  local i raw parsed actual
  for i in $(seq 1 "$attempts"); do
    raw="$(ask_actor "$actor" "$payload" 15)"
    assert_actor_ok "poll $actor $field" "$raw"
    parsed="$(extract_payload_json "$raw")"
    actual="$(PARSED="$parsed" FIELD="$field" python3 - <<'PY'
import json, os
value = json.loads(os.environ["PARSED"])
for part in os.environ["FIELD"].split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if isinstance(value, list):
    print(str(value))
elif value is None:
    print("")
else:
    print(value)
PY
)"
    if [ "$actual" = "$expected" ]; then
      return 0
    fi
    sleep "$sleep_s"
  done
  echo -e "${RED}Timed out waiting for $field=$expected on $actor${NC}"
  exit 1
}

echo "Step 0: Run native Python verification"
source "$HOME/venv/bin/activate" 2>/dev/null || true
python3 "$SCRIPT_DIR/verify.py"
echo ""

echo "Step 1: Ensure WASM artifact exists"
if [ ! -f "$WASM_FILE" ]; then
  "$SCRIPT_DIR/build.sh"
else
  echo "Using existing $WASM_FILE"
fi
echo ""

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://$HTTP_HOST:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start a PlexSpaces node and re-run this test${NC}"
  exit 1
fi

echo "Step 2: Deploy application"
curl -s -X DELETE "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/deploy" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -F "application_id=$APP_ID" \
  -F "name=chat-room-large-scale" \
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

sleep 1

echo ""
echo "Step 3: Test GuildActor — create channel"
echo "  Creating general channel in guild-acme..."
R="$(ask_actor "$GUILD_ACTOR" '{"op":"create_channel","channel_id":"general"}' 20)"
assert_actor_ok "create_channel" "$R"
echo "  ✓ create_channel OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 4: Test SessionActor — connect alice"
echo "  Connecting alice to guild-acme/general..."
R="$(ask_actor "$ALICE_SESSION" '{"op":"connect","user_id":"alice","guild_id":"guild-acme","channels":["general"]}' 20)"
assert_actor_ok "alice connect" "$R"
echo "  ✓ alice connect OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 5: Test ChannelActor — post message"
echo "  Posting message from alice to general channel..."
R="$(ask_actor "$CHANNEL_ACTOR" '{"op":"post_message","user_id":"alice","text":"Hello from alice","session_id":"alice-mobile"}' 20)"
assert_actor_ok "post_message" "$R"
echo "  ✓ post_message OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 6: Test ChannelActor — channel history"
echo "  Fetching channel history..."
R="$(ask_actor "$CHANNEL_ACTOR" '{"op":"history","limit":5}' 20)"
assert_actor_ok "channel history" "$R"
echo "  ✓ channel history OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 7: Test PresenceActor — set and get presence"
echo "  Setting alice presence to online..."
R="$(ask_actor "$PRESENCE_ACTOR" '{"op":"set_presence","user_id":"alice","guild_id":"guild-acme","status":"online","ttl_ms":60000}' 20)"
assert_actor_ok "set_presence" "$R"
echo "  ✓ set_presence OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 8: Test ConnectionFSM — transition and status"
echo "  Transitioning FSM to connected..."
R="$(ask_actor "$FSM_ACTOR" '{"op":"transition","to":"connected"}' 20)"
assert_actor_ok "fsm transition connected" "$R"
echo "  ✓ fsm transition connected OK: $(echo "$R" | head -c 100)"

echo "  Checking FSM status..."
R="$(ask_actor "$FSM_ACTOR" '{"op":"status"}' 20)"
assert_actor_ok "fsm status" "$R"
echo "  ✓ fsm status OK: $(echo "$R" | head -c 100)"

echo ""
echo -e "${GREEN}Python large-scale chat example passed${NC}"
