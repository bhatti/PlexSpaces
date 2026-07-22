#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Weather Actor (Python WASM): get_weather, cache_stats, clear_cache.
# Usage:
#   ./test.sh [HTTP_PORT]     # contract tests + full example run
#   ./test.sh --contract-only # contract tests only
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SDK_DIR="$PROJECT_ROOT/sdks/python"
WASM_FILE="$SCRIPT_DIR/weather_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="weather-python-test"
RUN_ID="$(date +%s)"

if [[ "${1:-}" == "--contract-only" ]]; then
  HTTP_PORT=""
fi

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

source "$PROJECT_ROOT/examples/rust/apps/test-common.sh"
source "$HOME/venv/bin/activate" 2>/dev/null || true

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "================================================================"
echo "  Weather Actor (Python WASM, service link + KV cache)"
echo "================================================================"

echo "Step 0: Contract tests"
PYTHONPATH="$SDK_DIR:$SCRIPT_DIR" python3 "$SCRIPT_DIR/test_weather_actor.py"
echo -e "  ${GREEN}Contract tests passed${NC}"
echo ""

if [[ -z "$HTTP_PORT" ]]; then
  echo -e "${GREEN}Contract-only run complete${NC}"
  exit 0
fi

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""


trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 2: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -F "application_id=$APP_ID" \
  -F "name=$APP_ID" \
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

WEATHER_ACTOR="weather:default"

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

assert_actor_ok() {
  local step_name="$1" json_raw="$2"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_raw" python3 - <<'PY'
import json, os, sys
d = json.loads(os.environ["ASSERT_JSON"])
def deep_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False:
            return str(obj.get("error") or obj.get("error_message") or "request failed")
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

echo ""
echo "Step 3: Test weather actor operations"
echo "  Checking cache stats (initial state)..."
R="$(send_actor "$WEATHER_ACTOR" '{"op":"cache_stats"}' 20)"
assert_actor_ok "cache_stats initial" "$R"
echo "  ✓ cache_stats OK: $(echo "$R" | head -c 100)"

echo "  Fetching weather for London..."
R="$(send_actor "$WEATHER_ACTOR" '{"op":"get_weather","city":"London"}' 20)"
assert_actor_ok "get_weather London" "$R"
echo "  ✓ get_weather London OK: $(echo "$R" | head -c 100)"

echo "  Fetching weather again (should be cached)..."
R="$(send_actor "$WEATHER_ACTOR" '{"op":"get_weather","city":"London"}' 20)"
assert_actor_ok "get_weather London cached" "$R"
echo "  ✓ get_weather London (cached) OK: $(echo "$R" | head -c 100)"

echo "  Checking cache stats (should show hits/misses)..."
R="$(send_actor "$WEATHER_ACTOR" '{"op":"cache_stats"}' 20)"
assert_actor_ok "cache_stats after fetch" "$R"
echo "  ✓ cache_stats OK: $(echo "$R" | head -c 100)"

echo "  Clearing cache..."
R="$(send_actor "$WEATHER_ACTOR" '{"op":"clear_cache"}' 20)"
assert_actor_ok "clear_cache" "$R"
echo "  ✓ clear_cache OK: $(echo "$R" | head -c 100)"

echo ""
echo "================================================================"
echo -e "  ${GREEN}Weather Actor (Python) example test complete${NC}"
echo "================================================================"
