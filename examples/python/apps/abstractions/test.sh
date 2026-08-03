#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/python/abstractions/abstractions_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="abstractions-python"
ABSTRACTIONS_ACTOR="cart-1:abstractions"
EPHEMERAL_ACTOR="session-1:ephemeral"
WORKFLOW_ACTOR="order-1:workflow"
CHANNEL_ACTOR="alerts:channel"
CONTROLLER_ACTOR="controller"

GREEN='\033[0;32m'
RED='\033[0;31m'
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
echo "AUTH_HEADER set: $([ -n "$AUTH_HEADER" ] && echo 'YES' || echo 'NO')"

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

echo "Step 0: Run native Python verification"
source "$HOME/venv/bin/activate" 2>/dev/null || true
python3 "$SCRIPT_DIR/verify.py"
echo ""

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node with ./scripts/server.sh and re-run this test${NC}"
  exit 1
fi

echo "Step 2: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  echo "  Deploy attempt $_attempt (timeout 500s)..."
  CURL_EXIT=0
  if [ -n "$AUTH_HEADER" ]; then
    DEPLOY_OUT=$(curl --max-time 500 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -H "$AUTH_HEADER" \
    -F "application_id=$APP_ID" \
    -F "name=abstractions-python" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || CURL_EXIT=$?
  else
    APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
    zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
    DEPLOY_OUT=$(curl --max-time 500 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=abstractions-python" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || CURL_EXIT=$?
  fi
  if [ "$CURL_EXIT" -ne 0 ]; then
    echo -e "  ${RED}curl failed with exit code $CURL_EXIT${NC}"
    echo "  Output: $(echo "$DEPLOY_OUT" | tail -5)"
    sleep 3
    continue
  fi
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  echo "  HTTP $HTTP_CODE — $(echo "$RESPONSE" | grep -o '"success"[^,}]*' | head -1)"
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo -e "  ${RED}Deploy attempt $_attempt FAILED${NC}"
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}FATAL: Deploy failed after 3 attempts. Last HTTP $HTTP_CODE${NC}"
  echo "  Response: $RESPONSE"
  exit 1
fi
echo -e "  ${GREEN}Deploy successful${NC}"

echo "Step 9: Non-durable reactivation"
echo "  Checking ephemeral actor status..."
EPHEMERAL_UPDATED="$(send_actor "$EPHEMERAL_ACTOR" '{"op":"status"}' 15)"
assert_actor_ok "ephemeral status" "$EPHEMERAL_UPDATED"
echo "  ✓ ephemeral status OK"
echo "  Incrementing ephemeral actor..."
assert_actor_ok "ephemeral increment" "$(send_actor "$EPHEMERAL_ACTOR" '{"op":"increment","amount":2}' 15)"
echo "  ✓ ephemeral increment OK"
# Stop via DELETE API so the Rust node handles stop directly (no WASM controller indirection).
# DELETE /api/v1/actors/{namespace}/{actor_type:id} resolves canonical ID, calls actor_factory.stop_actor,
# then prime_instance_from_definition refreshes the spec so reactivation uses initial_count=5.
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
echo "  ✓ ephemeral actor stopped"
echo "  Waiting for reactivation (count should reset to 5)..."
wait_for_json_field \
  "$EPHEMERAL_ACTOR" \
  '{"op":"status"}' \
  "count" \
  "5" \
  30 \
  0.2
echo "  ✓ ephemeral reactivated with count=5"

echo ""
echo -e "${GREEN}Python abstractions example passed — all assertions verified${NC}"
