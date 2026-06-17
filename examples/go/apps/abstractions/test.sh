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

HTTP_CHECK=$(curl -s --max-time 5 -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node with ./scripts/server.sh and re-run this test${NC}"
  exit 1
fi

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
    -F "name=abstractions-go" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
  else
    DEPLOY_OUT=$(curl -s --max-time 500 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=abstractions-go" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
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
echo -e "  ${GREEN}Deploy successful${NC}"

echo ""
echo "Step 3: Test abstractions actor (GenServer)"
echo "  Sending status..."
R="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"status"}' 20)"
assert_actor_ok "abstractions status" "$R"
echo "  ✓ status OK: $(echo "$R" | head -c 100)"

echo "  Incrementing..."
R="$(send_actor "$ABSTRACTIONS_ACTOR" '{"op":"increment","amount":3}' 20)"
assert_actor_ok "abstractions increment" "$R"
echo "  ✓ increment OK"

echo "  Verifying count=3..."
wait_for_json_field "$ABSTRACTIONS_ACTOR" '{"op":"status"}' "count" "3" 15 0.3
echo "  ✓ count=3 confirmed"

echo ""
echo "Step 4: Test ephemeral (non-durable) actor"
echo "  Checking ephemeral status..."
R="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 20)"
assert_actor_ok "ephemeral status" "$R"
echo "  ✓ ephemeral status OK"

echo "  Incrementing ephemeral..."
assert_actor_ok "ephemeral increment" "$(send_actor "$EPHEMERAL_ACTOR" '{"op":"increment","amount":2}' 20)"
echo "  ✓ ephemeral increment OK"

echo "  Stopping ephemeral actor..."
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
echo "  ✓ ephemeral stopped"

echo "  Waiting for reactivation (count should reset to init value)..."
wait_for_json_field "$EPHEMERAL_ACTOR" '{"op":"status"}' "count" "5" 30 0.3
echo "  ✓ ephemeral reactivated with initial count=5"

echo ""
echo -e "${GREEN}Go abstractions example passed — all assertions verified${NC}"
