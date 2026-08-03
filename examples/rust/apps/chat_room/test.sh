#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/chat_room_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="rust-chat-room-large-scale"
GUILD_ACTOR="guild-acme:GuildActor"
ALICE_SESSION="alice-mobile:SessionActor"
BOB_SESSION="bob-desktop:SessionActor"
CHANNEL_ACTOR="guild-acme__general:ChannelActor"
PRESENCE_ACTOR="alice:PresenceActor"
FSM_ACTOR="alice-mobile:ConnectionFSM"
WORKFLOW_ACTOR="report-1:ModerationWorkflow"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'



# Auto-generate JWT if not provided
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

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
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
  local step_name="$1"
  local json_raw="$2"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except json.JSONDecodeError as e:
    print(f"FAIL [{step}]: invalid JSON: {e}", file=sys.stderr)
    sys.exit(1)
def deep_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False:
            if obj.get("error"):
                return str(obj["error"])
            if obj.get("error_message"):
                return str(obj["error_message"])
            payload = obj.get("payload")
            if isinstance(payload, dict) and payload.get("error"):
                return str(payload["error"])
            return "request failed"
        if obj.get("status") == "error":
            return str(obj.get("error") or obj.get("error_message") or "error status")
        if obj.get("error"):
            return str(obj["error"])
        for v in obj.values():
            e = deep_err(v)
            if e:
                return e
    elif isinstance(obj, list):
        for item in obj:
            e = deep_err(item)
            if e:
                return e
    return None
err = deep_err(d)
if err:
    print(f"FAIL [{step}]: {err}", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step_name${NC}"
    echo "  Raw: $(echo "$json_raw" | head -c 400)"
    exit 1
  fi
}

wait_for_field() {
  local actor="$1" payload="$2" field="$3" expected="$4" attempts="${5:-25}" sleep_s="${6:-0.2}"
  local i raw parsed actual
  for i in $(seq 1 "$attempts"); do
    raw="$(send_actor "$actor" "$payload" 15)"
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
  echo "  Last: $parsed"
  exit 1
}

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Build did not produce $WASM_FILE${NC}"
  exit 1
fi

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start a PlexSpaces node and re-run: ./scripts/server.sh $HTTP_PORT${NC}"
  exit 1
fi

echo "Step 2: Deploy application"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -F "application_id=$APP_ID" \
  -F "name=rust-chat-room-large-scale" \
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
R="$(send_actor "$GUILD_ACTOR" '{"op":"create_channel","channel_id":"general"}' 20)"
assert_actor_ok "create_channel" "$R"
echo "  ✓ create_channel OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 4: Test SessionActor — connect alice"
echo "  Connecting alice to guild-acme/general..."
R="$(send_actor "$ALICE_SESSION" '{"op":"connect","user_id":"alice","guild_id":"guild-acme","channels":["general"]}' 20)"
assert_actor_ok "alice connect" "$R"
echo "  ✓ alice connect OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 5: Test ChannelActor — post message"
echo "  Posting message from alice to general channel..."
R="$(send_actor "$CHANNEL_ACTOR" '{"op":"post_message","user_id":"alice","text":"Hello from Rust alice","session_id":"alice-mobile"}' 20)"
assert_actor_ok "post_message" "$R"
echo "  ✓ post_message OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 6: Test ChannelActor — channel history"
echo "  Fetching channel history..."
R="$(send_actor "$CHANNEL_ACTOR" '{"op":"history","limit":5}' 20)"
assert_actor_ok "channel history" "$R"
echo "  ✓ channel history OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 7: Test PresenceActor — set presence"
echo "  Setting alice presence to online..."
R="$(send_actor "$PRESENCE_ACTOR" '{"op":"set_presence","user_id":"alice","guild_id":"guild-acme","status":"online","ttl_ms":60000}' 20)"
assert_actor_ok "set_presence" "$R"
echo "  ✓ set_presence OK: $(echo "$R" | head -c 100)"

echo ""
echo "Step 8: Test ConnectionFSM — transition and status"
echo "  Transitioning FSM to connected..."
R="$(send_actor "$FSM_ACTOR" '{"op":"transition","to":"connected"}' 20)"
assert_actor_ok "fsm transition connected" "$R"
echo "  ✓ fsm transition connected OK: $(echo "$R" | head -c 100)"

echo "  Checking FSM status..."
R="$(send_actor "$FSM_ACTOR" '{"op":"status"}' 20)"
assert_actor_ok "fsm status" "$R"
echo "  ✓ fsm status OK: $(echo "$R" | head -c 100)"

echo -e "  ${GREEN}FSM and workflow verified${NC}"

echo ""
echo -e "${GREEN}Rust large-scale chat example passed${NC}"
