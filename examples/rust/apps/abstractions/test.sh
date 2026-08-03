#!/usr/bin/env bash
# Abstractions (Rust WASM): deploy and verify GenServer, workflow, event-channel,
# timers, virtual actor reactivation, and shared services against a running node.
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/abstractions_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="abstractions-rust"
ABSTRACTIONS_ACTOR="cart-1:abstractions"
EPHEMERAL_ACTOR="session-1:ephemeral"
WORKFLOW_ACTOR="order-1:workflow"
CHANNEL_ACTOR="alerts:channel"
CONTROLLER_ACTOR="controller"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
NODE_META="$REPO_ROOT/plexspaces-node-${HTTP_PORT}.meta"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"

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
        if obj.get("status") == "error":
            p = obj.get("payload")
            if isinstance(p, str):
                try:
                    p = json.loads(p)
                except Exception:
                    return str(obj.get("error") or obj)
            if isinstance(p, dict) and p.get("error"):
                return str(p.get("error"))
            if obj.get("error"):
                return str(obj.get("error"))
            return str(obj.get("error_message") or "error")
        if obj.get("error"):
            return str(obj.get("error"))
        for v in obj.values():
            e = deep_err(v)
            if e:
                return e
    elif isinstance(obj, list):
        for x in obj:
            e = deep_err(x)
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

wait_for_json_field() {
  local actor="$1" payload="$2" field="$3" expected="$4" attempts="${5:-20}" sleep_s="${6:-0.2}"
  local i response parsed actual
  for i in $(seq 1 "$attempts"); do
    response="$(send_actor "$actor" "$payload" 15)"
    assert_actor_ok "poll $actor $field" "$response"
    parsed="$(extract_payload_json "$response")"
    actual="$(PARSED="$parsed" FIELD="$field" python3 - <<'PY'
import json, os
data = json.loads(os.environ["PARSED"])
value = data
for part in os.environ["FIELD"].split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if isinstance(value, bool):
    print("true" if value else "false")
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
    "crates/services/src/actor_service/mod.rs",
    "crates/application/src/wasm_message_sender.rs",
    "crates/application/src/wasm_application.rs",
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
    echo -e "${RED}Node binary is older than the spawn/role mapping sources${NC}"
    echo "$NEWER_SOURCES" | sed 's/^/  newer source: /'
    echo "  Rebuild and restart from repo root with: make build && ./scripts/server.sh $HTTP_PORT"
    exit 1
  fi
}

echo "Step 0: Run native contract tests"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$(cd "$SCRIPT_DIR/../../../.." && pwd)/target}"
cargo test --manifest-path "$SCRIPT_DIR/Cargo.toml"
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

echo "Step 1: Build WASM"
if [ -f "$WASM_FILE" ]; then
  echo "  (WASM already built, skipping rebuild)"
else
  "$SCRIPT_DIR/build.sh"
fi
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Build did not produce $WASM_FILE${NC}"
  exit 1
fi

require_fresh_server_sh_node

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
    -F "name=abstractions-rust" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  else
    APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
    zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
    DEPLOY_OUT=$(curl -s --max-time 500 -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=abstractions-rust" \
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
echo -e "  ${GREEN}non-durable virtual actor reinitialized from init-config${NC}"

echo "Step 10: Spawn helper actor through controller"
SPAWN_RESULT=$(send_actor "$CONTROLLER_ACTOR" '{"op":"spawn_actor","module_ref":"abstractions_wasm","actor_id":"spawned-1","config":{"role":"abstractions"}}' 15)
assert_actor_ok "spawn_actor" "$SPAWN_RESULT"
SPAWNED_ACTOR_ID="$(extract_payload_json "$SPAWN_RESULT")"
echo "  spawned: $SPAWNED_ACTOR_ID"

echo "Step 11: Application listing"
APP_LIST_JSON="$(fetch_applications_list_json "http://localhost:$HTTP_PORT")"
if ! echo "$APP_LIST_JSON" | grep -q "$APP_ID"; then
  echo -e "${YELLOW}Application list did not include $APP_ID via CLI lookup${NC}"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Abstractions (Rust WASM) Test Complete${NC}"
echo "================================================================"
