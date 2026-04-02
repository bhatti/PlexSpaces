#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/typescript/abstractions/abstractions_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"
APP_ID="abstractions-typescript"
ABSTRACTIONS_ACTOR="abstractions:cart-1"
EPHEMERAL_ACTOR="ephemeral:session-1"
WORKFLOW_ACTOR="workflow:order-1"
CHANNEL_ACTOR="channel:alerts"
CONTROLLER_ACTOR="controller"

GREEN='\033[0;32m'
RED='\033[0;31m'
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
d = json.loads(os.environ["ASSERT_JSON"])
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
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: {err}", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step_name${NC}"
    echo "  Raw: $(echo "$json_raw" | head -c 400)"
    exit 1
  fi
}

wait_for_json_field() {
  local actor="$1" payload="$2" field="$3" expected="$4" attempts="${5:-20}" sleep_s="${6:-0.2}"
  local i response parsed actual
  for i in $(seq 1 "$attempts"); do
    response="$(send_actor "$actor" "$payload" 15)"
    assert_actor_ok "poll $actor $field" "$response"
    parsed="$(extract_payload_json "$response")"
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

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node with ./scripts/server.sh and re-run this test${NC}"
  exit 1
fi

echo "Step 2: Deploy"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=abstractions-typescript" \
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

echo "Step 3: Durable virtual actor"
assert_actor_ok "increment" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"increment","amount":2}' 15)"
STATUS="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "status" "$STATUS"
ACTOR_PAYLOAD="$(extract_payload_json "$STATUS")"
INTERNAL_ACTOR_ID="$(PARSED="$ACTOR_PAYLOAD" python3 - <<'PY'
import json, os
print(json.loads(os.environ["PARSED"]).get("self_id", ""))
PY
)"

echo "Step 4: Timers and reminders"
assert_actor_ok "schedule_timer" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"schedule_timer","delay_ms":100}' 15)"
assert_actor_ok "schedule_reminder" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"schedule_reminder","delay_ms":140}' 15)"
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "timer_ticks" "1" 25 0.15
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "reminder_ticks" "1" 25 0.15

echo "Step 5: Workflow"
assert_actor_ok "workflow_run" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_run","order_id":"o-1"}' 15)"
assert_actor_ok "workflow_signal" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_signal:cancel","reason":"user"}' 15)"
assert_actor_ok "workflow_query" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_query:status"}' 15)"

echo "Step 6: Process groups"
assert_actor_ok "send_event" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"send_event","target":"channel:alerts","channel":"alerts","body":"direct"}' 15)"
assert_actor_ok "broadcast_event" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"broadcast_event","group":"abstractions-group","channel":"alerts","body":"broadcast"}' 15)"

echo "Step 7: Durable reactivation"
STOP_DURABLE_PAYLOAD="$(printf '{"op":"stop_actor","actor_id":"%s"}' "$INTERNAL_ACTOR_ID")"
assert_actor_ok "stop_actor" "$(send_actor "$CONTROLLER_ACTOR" "$STOP_DURABLE_PAYLOAD" 15)"
sleep 1
REACTIVATED="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "reactivated status" "$REACTIVATED"
if ! echo "$(extract_payload_json "$REACTIVATED")" | grep -q '"count": 2\|"count":2'; then
  echo -e "${RED}Virtual actor did not preserve durable state after reactivation${NC}"
  exit 1
fi

echo "Step 8: Non-durable reactivation"
EPHEMERAL_UPDATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral status" "$EPHEMERAL_UPDATED"
assert_actor_ok "ephemeral increment" "$(send_actor "$EPHEMERAL_ACTOR" '{"op":"increment","amount":2}' 15)"
EPHEMERAL_UPDATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
EPHEMERAL_INTERNAL_ID="$(PARSED="$(extract_payload_json "$EPHEMERAL_UPDATED")" python3 - <<'PY'
import json, os
print(json.loads(os.environ["PARSED"]).get("self_id", ""))
PY
)"
STOP_EPHEMERAL_PAYLOAD="$(printf '{"op":"stop_actor","actor_id":"%s"}' "$EPHEMERAL_INTERNAL_ID")"
assert_actor_ok "stop_ephemeral" "$(send_actor "$CONTROLLER_ACTOR" "$STOP_EPHEMERAL_PAYLOAD" 15)"
sleep 1
EPHEMERAL_REACTIVATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral reactivated" "$EPHEMERAL_REACTIVATED"
if ! echo "$(extract_payload_json "$EPHEMERAL_REACTIVATED")" | grep -q '"count": 5\|"count":5'; then
  echo -e "${RED}Non-durable virtual actor did not reset to init-config after reactivation${NC}"
  exit 1
fi

echo ""
echo -e "${GREEN}TypeScript abstractions example passed${NC}"
