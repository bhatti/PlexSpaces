#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/mcp_tool_server_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"


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

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

APP_ID="python-mcp-tool-server"
APP_NAME="python-mcp-tool-server"
# actor_type must match the Python class name in app-config.toml exactly.
REGISTRY_ACTOR="ToolRegistryActor:default"
TEMP_CONFIG=""

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"


grpc_seed_nodes() {
  local seed_list=()
  local joined=""
  local sep=""
  for entry in ${seed_list[@]+"${seed_list[@]}"}; do
    joined="${joined}${sep}${entry}"
    sep=", "
  done
  printf '%s' "$joined"
}

render_config() {
  local temp_config="$1"
  local seeds
  seeds="$(grpc_seed_nodes)"
  python3 - "$CONFIG_FILE" "$temp_config" "$seeds" <<'PY'
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text()
seed_nodes = sys.argv[3]
lines = []
seed_replaced = False
for line in source.splitlines():
    if line.startswith("seed_nodes = "):
        lines.append(f"seed_nodes = [{seed_nodes}]")
        seed_replaced = True
    else:
        lines.append(line)
if not seed_replaced:
    lines.insert(3, f"seed_nodes = [{seed_nodes}]")
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY
}

fail() {
  echo -e "${RED}FAIL: $*${NC}" >&2
  exit 1
}

pass() {
  echo -e "${GREEN}PASS: $*${NC}"
}

# ── Build if needed ──────────────────────────────────────────────────────────
if [ ! -f "$WASM_FILE" ]; then
  "$SCRIPT_DIR/build.sh"
fi

# ── Connectivity check ───────────────────────────────────────────────────────
for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"
  p="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot connect to node at ${h}:${p}${NC}"
    exit 1
  fi
done

# ── Deploy ───────────────────────────────────────────────────────────────────
TEMP_CONFIG="$(mktemp -t python-mcp-tool-server-app-config)"
  trap 'rm -f "${APP_ZIP:-}" "${TEMP_CONFIG:-}"' EXIT
  APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
render_config "$TEMP_CONFIG"

echo "Undeploy from all nodes, then deploy to entry node..."
"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2
_deployed=0
for _attempt in 1 2 3; do
    response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
      "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "app_file=@$APP_ZIP" 2>&1) || true
  http_code=$(echo "$response" | tail -n1)
  body=$(echo "$response" | sed '$d')
  if [ "$http_code" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $body${NC}"
  exit 1
fi
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
sleep 2

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Running MCP Tool Server integration tests"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

BASE="http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID"

# JSON assertions use RESP in the environment. Do not pipe into `python3 - <<'PY'`;
# the heredoc supplies the program on stdin, so sys.stdin.read() would be empty.

# ── Test 1: tools/list — verify 3 built-in tools returned ───────────────────
echo ""
echo "Test 1: tools_list — list all available tools"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"tools_list"}')
RESP="$RESP" python3 <<'PY' || fail "tools_list: expected 3 tools"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
tools = payload.get("tools", [])
count = payload.get("count", len(tools))
assert count >= 3, f"expected >= 3 tools, got {count}: {[t.get('name') for t in tools]}"
names = {t["name"] for t in tools}
for expected in ("calculator", "search", "weather"):
    assert expected in names, f"expected tool '{expected}' in tools list"
print(f"  tools={sorted(names)} count={count}")
PY
pass "tools_list returned >= 3 tools including calculator, search, weather"

# ── Test 2: tools_call calculator — add(10, 5) == 15 ────────────────────────
echo ""
echo "Test 2: tools_call calculator — add(10, 5)"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"tools_call","tool_name":"calculator","input":{"operation":"add","a":10,"b":5}}')
RESP="$RESP" python3 <<'PY' || fail "calculator add(10,5): expected result=15"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert "error" not in payload, f"unexpected error: {payload}"
result = payload.get("result")
assert result == 15 or abs(float(result) - 15.0) < 1e-9, f"expected result=15, got {result}"
print(f"  result={result} operation={payload.get('operation')}")
PY
pass "calculator add(10,5) returned result=15"

# ── Test 3: tools_call calculator — divide by zero returns error ─────────────
echo ""
echo "Test 3: tools_call calculator — divide by zero"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"tools_call","tool_name":"calculator","input":{"operation":"divide","a":42,"b":0}}')
RESP="$RESP" python3 <<'PY' || fail "calculator divide by zero: expected error field"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert "error" in payload, f"expected error for divide-by-zero, got: {payload}"
assert payload["error"] == "division_by_zero", f"expected error=division_by_zero, got: {payload['error']}"
print(f"  error={payload['error']} message={payload.get('message','')}")
PY
pass "calculator divide-by-zero returned expected error"

# ── Test 4: tools_call search — query "actor", expect results ────────────────
echo ""
echo "Test 4: tools_call search — query 'actor'"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"tools_call","tool_name":"search","input":{"query":"actor","max_results":10}}')
RESP="$RESP" python3 <<'PY' || fail "search 'actor': expected at least 1 result"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert "error" not in payload, f"unexpected error: {payload}"
results = payload.get("results", [])
count = payload.get("count", len(results))
assert count >= 1, f"expected >= 1 search result for 'actor', got {count}"
print(f"  query={payload.get('query')} count={count}")
for r in results[:3]:
    print(f"    [{r['id']}] {r['content'][:60]}...")
PY
pass "search 'actor' returned actor-related documents"

# ── Test 5: tools_call weather — London celsius ──────────────────────────────
echo ""
echo "Test 5: tools_call weather — London celsius"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"tools_call","tool_name":"weather","input":{"location":"London","units":"celsius"}}')
RESP="$RESP" python3 <<'PY' || fail "weather London celsius: expected temperature field"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert "error" not in payload, f"unexpected error: {payload}"
assert "temperature" in payload, f"expected temperature field in: {payload}"
assert payload.get("location") == "London", f"expected location=London, got: {payload.get('location')}"
assert payload.get("units") == "celsius", f"expected units=celsius, got: {payload.get('units')}"
print(f"  location={payload['location']} temperature={payload['temperature']} condition={payload.get('condition')} humidity={payload.get('humidity')}")
PY
pass "weather London celsius returned temperature and weather data"

# ── Test 6: get_stats — verify invocation counts match calls made ────────────
echo ""
echo "Test 6: get_stats — verify invocation counts"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"get_stats"}')
RESP="$RESP" python3 <<'PY' || fail "get_stats: expected non-zero invocation counts"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
stats = payload.get("stats", {})
total = payload.get("total_invocations", 0)
assert total >= 3, f"expected at least 3 total invocations (calculator×2 + search + weather), got {total}"
# calculator was called twice (add + divide), search once, weather once
calc_invocations = stats.get("calculator", {}).get("invocations", 0)
assert calc_invocations >= 2, f"expected >= 2 calculator invocations, got {calc_invocations}"
search_invocations = stats.get("search", {}).get("invocations", 0)
assert search_invocations >= 1, f"expected >= 1 search invocation, got {search_invocations}"
weather_invocations = stats.get("weather", {}).get("invocations", 0)
assert weather_invocations >= 1, f"expected >= 1 weather invocation, got {weather_invocations}"
print(f"  total_invocations={total} total_errors={payload.get('total_errors',0)}")
for tool, info in sorted(stats.items()):
    print(f"  {tool}: invocations={info.get('invocations',0)} errors={info.get('errors',0)}")
PY
pass "get_stats returned correct invocation counts"

# ── Test 7: register_tool — add new tool, verify in tools_list ───────────────
echo ""
echo "Test 7: register_tool — add 'translator' tool"
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{
    "op":"register_tool",
    "tool_schema":{
      "name":"translator",
      "description":"Translate text between languages",
      "inputSchema":{
        "type":"object",
        "properties":{
          "text":{"type":"string"},
          "target_language":{"type":"string"}
        },
        "required":["text","target_language"]
      }
    }
  }')
RESP="$RESP" python3 <<'PY' || fail "register_tool: expected ok=true"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert payload.get("ok") is True, f"expected ok=true, got: {payload}"
assert payload.get("registered") == "translator", f"expected registered=translator, got: {payload}"
total = payload.get("total_tools", 0)
assert total >= 4, f"expected >= 4 tools after registering, got {total}"
print(f"  registered={payload['registered']} total_tools={total}")
PY
pass "register_tool accepted new tool"

# Verify translator appears in tools_list
RESP=$(curl -s --max-time 30 -X POST \
  "${BASE}/${REGISTRY_ACTOR}/ask?timeout=30" \
  -H "Content-Type: application/json" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -d '{"op":"tools_list"}')
RESP="$RESP" python3 <<'PY' || fail "tools_list after register: expected translator in list"
import json, os
raw = json.loads(os.environ["RESP"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
names = {t["name"] for t in payload.get("tools", [])}
assert "translator" in names, f"expected 'translator' in tools list, got: {sorted(names)}"
print(f"  tools={sorted(names)}")
PY
pass "tools_list includes newly registered 'translator' tool"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}All MCP Tool Server tests passed.${NC}"
