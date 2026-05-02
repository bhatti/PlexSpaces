#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/go/abstractions/abstractions_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="abstractions-go"
ABSTRACTIONS_ACTOR="abstractions:cart-1"
EPHEMERAL_ACTOR="ephemeral:session-1"
WORKFLOW_ACTOR="workflow:order-1"
CHANNEL_ACTOR="channel:alerts"
CONTROLLER_ACTOR="controller"

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
            if obj.get("error"):
                return str(obj.get("error"))
            if obj.get("error_message"):
                return str(obj.get("error_message"))
        if obj.get("error"):
            return str(obj.get("error"))
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
      echo "$parsed"
      return 0
    fi
    sleep "$sleep_s"
  done
  echo -e "${RED}Timed out waiting for $field=$expected on $actor${NC}"
  echo "  Last payload: $parsed"
  exit 1
}

echo "Step 0: Run native Go tests"
(
  cd "$SCRIPT_DIR"
  GOCACHE="${GOCACHE:-/tmp/plexspaces-go-cache}" go test ./...
)
echo ""

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Build did not produce $WASM_FILE${NC}"
  exit 1
fi

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
  -F "name=abstractions-go" \
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

echo "Step 3: GenServer virtual actor"
INC=$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"increment","amount":2}' 15)
assert_actor_ok "increment" "$INC"
STATUS=$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"status"}' 15)
assert_actor_ok "status" "$STATUS"
ACTOR_PAYLOAD="$(extract_payload_json "$STATUS")"
INTERNAL_ACTOR_ID="$(PARSED="$ACTOR_PAYLOAD" python3 - <<'PY'
import json, os
print(json.loads(os.environ["PARSED"]).get("self_id", ""))
PY
)"
COUNT_VALUE="$(PARSED="$ACTOR_PAYLOAD" python3 - <<'PY'
import json, os
print(json.loads(os.environ["PARSED"]).get("count", ""))
PY
)"
if [ "$COUNT_VALUE" != "2" ] || [ -z "$INTERNAL_ACTOR_ID" ]; then
  echo -e "${RED}Expected count=2 and a resolved self_id${NC}"
  echo "$ACTOR_PAYLOAD"
  exit 1
fi
echo -e "  ${GREEN}count=2 self_id=$INTERNAL_ACTOR_ID${NC}"

echo "Step 4: Timer and reminder delivery"
assert_actor_ok "schedule_timer" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"schedule_timer","delay_ms":100}' 15)"
assert_actor_ok "schedule_reminder" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"schedule_reminder","delay_ms":140}' 15)"
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "timer_ticks" "1" 25 0.15 >/dev/null
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "reminder_ticks" "1" 25 0.15 >/dev/null
echo -e "  ${GREEN}timer and reminder fired${NC}"

echo "Step 5: KV, tuple space, and blob services"
assert_actor_ok "kv_put" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"kv_put","key":"abstractions/config","value":"ready"}' 15)"
KV_GET="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"kv_get","key":"abstractions/config"}' 15)"
assert_actor_ok "kv_get" "$KV_GET"
if ! echo "$(extract_payload_json "$KV_GET")" | grep -q '"value": "ready"\|"value":"ready"'; then
  echo -e "${RED}KV get did not return ready${NC}"
  exit 1
fi
assert_actor_ok "ts_write" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"ts_write","tuple":["abstractions","task","t-1"]}' 15)"
TS_READ="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"ts_read","pattern":["abstractions","task","*"]}' 15)"
assert_actor_ok "ts_read" "$TS_READ"
if ! echo "$(extract_payload_json "$TS_READ")" | grep -q '"t-1"'; then
  echo -e "${RED}TupleSpace read did not return the expected tuple${NC}"
  exit 1
fi
assert_actor_ok "blob_upload" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"blob_upload","blob_id":"abstractions/blob-1","data":"aGVsbG8=","content_type":"text/plain"}' 15)"
BLOB_DOWN="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"blob_download","blob_id":"abstractions/blob-1"}' 15)"
assert_actor_ok "blob_download" "$BLOB_DOWN"
if ! echo "$(extract_payload_json "$BLOB_DOWN")" | grep -q '"data": "aGVsbG8="\|"data":"aGVsbG8="' ; then
  echo -e "${RED}Blob download did not return the expected content${NC}"
  exit 1
fi
echo -e "  ${GREEN}services verified${NC}"

echo "Step 6: Workflow actor"
assert_actor_ok "workflow_run" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_run","order_id":"o-1"}' 15)"
assert_actor_ok "workflow_signal" "$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_signal:cancel","reason":"user"}' 15)"
WF_STATUS="$(send_actor "$WORKFLOW_ACTOR" '{"op":"workflow_query:status"}' 15)"
assert_actor_ok "workflow_query" "$WF_STATUS"
if ! echo "$(extract_payload_json "$WF_STATUS")" | grep -q '"status": "cancelled"\|"status":"cancelled"'; then
  echo -e "${RED}Workflow status did not report cancelled${NC}"
  exit 1
fi
echo -e "  ${GREEN}workflow verified${NC}"

echo "Step 7: Event actor and process groups"
assert_actor_ok "channel status" "$(send_actor "$CHANNEL_ACTOR" '{"op":"status"}' 15)"
MEMBERS="$(send_actor "$CHANNEL_ACTOR" '{"op":"group_members","group":"abstractions-group"}' 15)"
assert_actor_ok "group_members" "$MEMBERS"
SEND_EVENT=$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"send_event","target":"channel:alerts","channel":"alerts","body":"direct"}' 15)
assert_actor_ok "send_event" "$SEND_EVENT"
BROADCAST=$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"broadcast_event","group":"abstractions-group","channel":"alerts","body":"broadcast"}' 15)
assert_actor_ok "broadcast_event" "$BROADCAST"
sleep 1
CHANNEL_FINAL="$(send_actor "$CHANNEL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "channel final status" "$CHANNEL_FINAL"
if ! echo "$(extract_payload_json "$CHANNEL_FINAL")" | grep -q 'alerts:direct'; then
  echo -e "${RED}Direct event delivery did not reach channel actor${NC}"
  exit 1
fi
if ! echo "$(extract_payload_json "$CHANNEL_FINAL")" | grep -q 'alerts:broadcast'; then
  echo -e "${RED}Broadcast event delivery did not reach channel actor${NC}"
  exit 1
fi
echo -e "  ${GREEN}event actor verified${NC}"

echo "Step 8: Stop and reactivate virtual actor"
STOP_DURABLE_PAYLOAD="$(printf '{"op":"stop_actor","actor_id":"%s"}' "$INTERNAL_ACTOR_ID")"
assert_actor_ok "stop_actor" "$(send_actor "$CONTROLLER_ACTOR" "$STOP_DURABLE_PAYLOAD" 15)"
sleep 1
REACTIVATED="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "reactivated status" "$REACTIVATED"
REACTIVATED_PAYLOAD="$(extract_payload_json "$REACTIVATED")"
if ! echo "$REACTIVATED_PAYLOAD" | grep -q '"count": 2\|"count":2'; then
  echo -e "${RED}Virtual actor did not preserve durable state after reactivation${NC}"
  echo "$REACTIVATED_PAYLOAD"
  exit 1
fi
echo -e "  ${GREEN}virtual actor reactivated with preserved state${NC}"

echo "Step 9: Stop and reactivate non-durable virtual actor"
EPHEMERAL_BEFORE="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral status before increment" "$EPHEMERAL_BEFORE"
if ! echo "$(extract_payload_json "$EPHEMERAL_BEFORE")" | grep -q '"count": 5\|"count":5'; then
  echo -e "${RED}Non-durable virtual actor did not start from init-config count${NC}"
  exit 1
fi
assert_actor_ok "ephemeral increment" "$(send_actor "$EPHEMERAL_ACTOR" '{"op":"increment","amount":2}' 15)"
EPHEMERAL_UPDATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral updated status" "$EPHEMERAL_UPDATED"
EPHEMERAL_UPDATED_PAYLOAD="$(extract_payload_json "$EPHEMERAL_UPDATED")"
EPHEMERAL_INTERNAL_ID="$(PARSED="$EPHEMERAL_UPDATED_PAYLOAD" python3 - <<'PY'
import json, os
print(json.loads(os.environ["PARSED"]).get("self_id", ""))
PY
)"
if ! echo "$EPHEMERAL_UPDATED_PAYLOAD" | grep -q '"count": 7\|"count":7'; then
  echo -e "${RED}Non-durable virtual actor did not update count before stop${NC}"
  echo "$EPHEMERAL_UPDATED_PAYLOAD"
  exit 1
fi
# Stop via DELETE API so the Rust node handles stop directly (no WASM controller indirection).
# DELETE /api/v1/actors/{namespace}/{actor_type:id} resolves canonical ID, calls actor_factory.stop_actor,
# then prime_instance_from_definition refreshes the spec so reactivation uses initial_count=5.
STOP_RESULT="$(curl -s --max-time 15 -X DELETE \
  "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$EPHEMERAL_ACTOR" \
  -H "Content-Type: application/json" 2>/dev/null || echo '{"error":"timeout"}')"
assert_actor_ok "stop_ephemeral" "$STOP_RESULT"
sleep 1
EPHEMERAL_REACTIVATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral reactivated status" "$EPHEMERAL_REACTIVATED"
EPHEMERAL_REACTIVATED_PAYLOAD="$(extract_payload_json "$EPHEMERAL_REACTIVATED")"
if ! echo "$EPHEMERAL_REACTIVATED_PAYLOAD" | grep -q '"count": 5\|"count":5'; then
  echo -e "${RED}Non-durable virtual actor did not reset to init-config after reactivation${NC}"
  echo "$EPHEMERAL_REACTIVATED_PAYLOAD"
  exit 1
fi
if ! echo "$EPHEMERAL_REACTIVATED_PAYLOAD" | grep -q '"role": "ephemeral"\|"role":"ephemeral"'; then
  echo -e "${RED}Non-durable virtual actor did not preserve init-config role after reactivation${NC}"
  echo "$EPHEMERAL_REACTIVATED_PAYLOAD"
  exit 1
fi
echo -e "  ${GREEN}non-durable virtual actor reinitialized from init-config${NC}"

echo ""
echo -e "${GREEN}Go abstractions example passed${NC}"
