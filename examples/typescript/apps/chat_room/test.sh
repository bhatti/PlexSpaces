#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/typescript/chat_room/chat_room_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="ts-chat-room-large-scale"
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

cleanup() {
  curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
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

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start a PlexSpaces node and re-run: ./scripts/server.sh $((HTTP_PORT - 1))${NC}"
  exit 1
fi

echo "Step 2: Deploy application"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=ts-chat-room-large-scale" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

echo "Step 3: Create guild channel"
assert_actor_ok "create_channel" "$(send_actor "$GUILD_ACTOR" '{"op":"create_channel","channel_id":"general"}' 15)"

echo "Step 4: Connect Alice and Bob"
assert_actor_ok "connect_alice" "$(send_actor "$ALICE_SESSION" '{"op":"connect","user_id":"alice","guild_id":"guild-acme","channels":["general"],"ttl_ms":5000}' 15)"
assert_actor_ok "connect_bob" "$(send_actor "$BOB_SESSION" '{"op":"connect","user_id":"bob","guild_id":"guild-acme","channels":["general"],"ttl_ms":5000}' 15)"

echo "Step 5: Alice sends a channel message"
assert_actor_ok "send_channel_message" "$(send_actor "$ALICE_SESSION" '{"op":"send_channel_message","channel_id":"general","text":"hello from alice"}' 15)"

echo "Step 6: Verify message history and fan-out delivery"
for _ in $(seq 1 30); do
  STORE_HISTORY="$(send_actor "$CHANNEL_ACTOR" '{"op":"history","limit":10}' 15)"
  assert_actor_ok "store_history" "$STORE_HISTORY"
  if echo "$(extract_payload_json "$STORE_HISTORY")" | grep -q '"count": 1\|"count":1'; then
    break
  fi
  sleep 0.2
done
STORE_PAYLOAD="$(extract_payload_json "$STORE_HISTORY")"
if ! echo "$STORE_PAYLOAD" | grep -q '"count": 1\|"count":1'; then
  echo -e "${RED}Expected one stored message in channel history${NC}"
  echo "$STORE_PAYLOAD"
  exit 1
fi
for _ in $(seq 1 30); do
  BOB_INBOX="$(send_actor "$BOB_SESSION" '{"op":"inbox"}' 15)"
  assert_actor_ok "bob_inbox" "$BOB_INBOX"
  if echo "$(extract_payload_json "$BOB_INBOX")" | grep -q '"from_user": "alice"\|"from_user":"alice"'; then
    break
  fi
  sleep 0.2
done
if ! echo "$(extract_payload_json "$BOB_INBOX")" | grep -q '"from_user": "alice"\|"from_user":"alice"'; then
  echo -e "${RED}Expected Bob to receive Alice's message via fan-out${NC}"
  exit 1
fi
echo -e "  ${GREEN}message stored and delivered${NC}"

echo "Step 7: Typing indicator expires"
assert_actor_ok "set_typing" "$(send_actor "$ALICE_SESSION" '{"op":"set_typing","channel_id":"general","ttl_ms":120}' 15)"
wait_for_field "$CHANNEL_ACTOR" '{"op":"status"}' "typing_users" "[]" 30 0.15
echo -e "  ${GREEN}typing indicator expired${NC}"

echo "Step 8: Presence expires"
assert_actor_ok "presence_set" "$(send_actor "$PRESENCE_ACTOR" '{"op":"set_presence","user_id":"alice","guild_id":"guild-acme","status":"away","ttl_ms":150}' 15)"
wait_for_field "$PRESENCE_ACTOR" '{"op":"status"}' "status" "offline" 30 0.15
echo -e "  ${GREEN}presence expired to offline${NC}"

echo "Step 9: Connection FSM and moderation workflow"
FSM_STATUS="$(send_actor "$FSM_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "fsm_status" "$FSM_STATUS"
FSM_PAYLOAD="$(extract_payload_json "$FSM_STATUS")"
if ! echo "$FSM_PAYLOAD" | grep -q '"state": "joined"\|"state":"joined"\|"state": "idle"\|"state":"idle"'; then
  echo -e "${YELLOW}FSM did not reach an expected steady state${NC}"
  echo "$FSM_PAYLOAD"
  exit 1
fi
assert_actor_ok "workflow_run" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_run","report_id":"report-1","message_id":"general-1","reporter_id":"bob","reason":"spam"}' 15)"
assert_actor_ok "workflow_review" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_signal:review","moderator_id":"mod-1","resolution":"warn"}' 15)"
WF_STATUS="$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_query:status"}' 15)"
assert_actor_ok "workflow_status" "$WF_STATUS"
if ! echo "$(extract_payload_json "$WF_STATUS")" | grep -q '"status": "reviewed"\|"status":"reviewed"'; then
  echo -e "${RED}Expected moderation workflow to reach reviewed status${NC}"
  exit 1
fi
echo -e "  ${GREEN}FSM and workflow verified${NC}"

echo ""
echo -e "${GREEN}TypeScript large-scale chat example passed${NC}"
