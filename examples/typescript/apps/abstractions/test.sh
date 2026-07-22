#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/typescript/abstractions/abstractions_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="abstractions-typescript"
ABSTRACTIONS_ACTOR="abstractions:cart-1"
EPHEMERAL_ACTOR="ephemeral:session-1"
WORKFLOW_ACTOR="workflow:order-1"
CHANNEL_ACTOR="channel:alerts"
CONTROLLER_ACTOR="controller"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'
NODE_META="$REPO_ROOT/plexspaces-node-${HTTP_PORT}.meta"

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
  echo "  Token: ${PLEXSPACES_TEST_TOKEN:0:20}..."
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi
echo "AUTH_HEADER set: $([ -n "$AUTH_HEADER" ] && echo YES || echo NO)"

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  if [ -n "$AUTH_HEADER" ]; then
    curl -s --max-time "$timeout" -X POST \
      "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
      -H "Content-Type: application/json" \
      -H "$AUTH_HEADER" \
      -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
  else
    curl -s --max-time "$timeout" -X POST \
      "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
      -H "Content-Type: application/json" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
  fi
}

extract_payload_json() {
  local raw="$1"
  RAW_RESPONSE="$raw" python3 - <<'PY'
import json, os
raw = os.environ["RAW_RESPONSE"]
data = json.loads(raw)
if not isinstance(data, dict):
    print(json.dumps({}))
    raise SystemExit(0)
payload = data.get("payload", data)
if payload is None:
    print(json.dumps({}))
    raise SystemExit(0)
if isinstance(payload, str):
    try:
        payload = json.loads(payload)
    except Exception:
        payload = {"value": payload}
if payload is None:
    payload = {}
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
  local actor="$1" payload="$2" field="$3" expected="$4" attempts="${5:-20}" sleep_s="${6:-0.2}" hint="${7:-}"
  local i response parsed actual last_actual=""
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
    last_actual="$actual"
    if [ "$actual" = "$expected" ]; then
      return 0
    fi
    sleep "$sleep_s"
  done
  echo -e "${RED}Timed out waiting for $field=$expected on $actor${NC}"
  if [ -n "$last_actual" ]; then
    echo "  Last observed $field=$last_actual"
  fi
  if [ -n "$hint" ]; then
    echo "  Hint: $hint"
  fi
  exit 1
}

current_binary_mtime_epoch() {
  python3 - <<'PY'
import os
print(int(os.path.getmtime("target/debug/plexspaces")))
PY
}

require_binary_newer_than_sources() {
  cd "$REPO_ROOT"
  python3 - <<'PY'
import os
import sys

binary = "target/debug/plexspaces"
sources = [
    "crates/node/src/wasm_apps_loader.rs",
    "crates/application/src/wasm_application.rs",
    "crates/actor/src/actor_factory_impl.rs",
    "crates/core/src/virtual_actor_manager.rs",
]

binary_mtime = os.path.getmtime(binary)
newer = [path for path in sources if os.path.exists(path) and os.path.getmtime(path) > binary_mtime]
if newer:
    print("\n".join(newer))
    sys.exit(1)
PY
}

require_fresh_server_sh_node() {
  if [ ! -f "$NODE_META" ]; then
    echo -e "${RED}Refusing to run against an untracked node on port $HTTP_PORT${NC}"
    echo "  Expected metadata file: $NODE_META"
    echo "  Start the node from repo root with: make build && ./scripts/server.sh $HTTP_PORT"
    exit 1
  fi

  local current_mtime meta_mtime meta_binary
  current_mtime="$(cd "$REPO_ROOT" && current_binary_mtime_epoch)"
  meta_mtime="$(grep '^binary_mtime_epoch=' "$NODE_META" | head -n1 | cut -d= -f2-)"
  meta_binary="$(grep '^binary_path=' "$NODE_META" | head -n1 | cut -d= -f2-)"

  if [ "$meta_binary" != "$REPO_ROOT/target/debug/plexspaces" ] || [ "$meta_mtime" != "$current_mtime" ]; then
    echo -e "${RED}Node on port $HTTP_PORT is not running the current repo build${NC}"
    echo "  Expected binary: $REPO_ROOT/target/debug/plexspaces"
    echo "  Recorded binary: ${meta_binary:-missing}"
    echo "  Expected mtime: $current_mtime"
    echo "  Recorded mtime: ${meta_mtime:-missing}"
    echo "  Rebuild and restart from repo root with: make build && ./scripts/server.sh $HTTP_PORT"
    exit 1
  fi

  if ! NEWER_SOURCES="$(require_binary_newer_than_sources)"; then
    echo -e "${RED}Node binary is older than role-mapping runtime sources${NC}"
    echo "$NEWER_SOURCES" | sed 's/^/  newer source: /'
    echo "  Rebuild and restart from repo root with: make build && ./scripts/server.sh $HTTP_PORT"
    exit 1
  fi
}

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s --max-time 5 -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node with ./scripts/server.sh and re-run this test${NC}"
  exit 1
fi

#require_fresh_server_sh_node

echo "Step 2: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  echo "  Deploy attempt $_attempt..."
  if [ -n "$AUTH_HEADER" ]; then
    DEPLOY_OUT=$(curl -s --max-time 500 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -H "$AUTH_HEADER" \
    -F "application_id=$APP_ID" \
    -F "name=abstractions-typescript" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  else
    APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
    zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
    DEPLOY_OUT=$(curl -s --max-time 500 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=abstractions-typescript" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  fi
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  echo "  HTTP $HTTP_CODE: $(echo "$RESPONSE" | head -c 300)"
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo -e "  ${RED}Deploy attempt $_attempt FAILED${NC}"
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi

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
# Increment count twice so we can verify journal persistence after stop+restart
assert_actor_ok "increment_1" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"increment","amount":1}' 15)"
assert_actor_ok "increment_2" "$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"increment","amount":1}' 15)"
# Verify count=2 before stopping
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "count" "2" 10 0.2
# Stop actor — durability facet should persist state to journal
if [ -n "$AUTH_HEADER" ]; then
  STOP_RESULT="$(curl -s --max-time 15 -X DELETE \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ABSTRACTIONS_ACTOR" \
    -H "Content-Type: application/json" \
    -H "$AUTH_HEADER" 2>/dev/null || echo '{"error":"timeout"}')"
else
  STOP_RESULT="$(curl -s --max-time 15 -X DELETE \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ABSTRACTIONS_ACTOR" \
    -H "Content-Type: application/json" 2>/dev/null || echo '{"error":"timeout"}')"
fi
assert_actor_ok "stop_actor" "$STOP_RESULT"
# After reactivation, count should be restored from journal
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "count" "2" 25 0.2

echo "Step 8: Non-durable reactivation"
EPHEMERAL_UPDATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral status" "$EPHEMERAL_UPDATED"
assert_actor_ok "ephemeral increment" "$(send_actor "$EPHEMERAL_ACTOR" '{"op":"increment","amount":2}' 15)"
# Stop via DELETE API so the Rust node handles stop directly (no WASM controller indirection).
# DELETE /api/v1/actors/{namespace}/{actor_type:id} resolves canonical ID, calls actor_factory.stop_actor,
# then prime_instance_from_definition refreshes the spec so reactivation uses initial_count=5.
if [ -n "$AUTH_HEADER" ]; then
  STOP_RESULT="$(curl -s --max-time 15 -X DELETE \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$EPHEMERAL_ACTOR" \
    -H "Content-Type: application/json" \
    -H "$AUTH_HEADER" 2>/dev/null || echo '{"error":"timeout"}')"
else
  STOP_RESULT="$(curl -s --max-time 15 -X DELETE \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$EPHEMERAL_ACTOR" \
    -H "Content-Type: application/json" 2>/dev/null || echo '{"error":"timeout"}')"
fi
assert_actor_ok "stop_ephemeral" "$STOP_RESULT"
wait_for_json_field \
  "$EPHEMERAL_ACTOR" \
  '{"op":"status"}' \
  "count" \
  "5" \
  30 \
  0.2 \
  "This reactivation path lives in the Rust node. Rebuild and restart the node from repo root with 'make build' and './scripts/server.sh 8091' before rerunning this test."

echo ""
echo -e "${GREEN}TypeScript abstractions example passed${NC}"
