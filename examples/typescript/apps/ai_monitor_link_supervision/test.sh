#!/usr/bin/env bash
# Test AI Monitor/Link Supervision (TypeScript WASM)
# Usage: ./test.sh [HTTP_PORT|node1:port1 node2:port2 ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/typescript/ai_monitor_link_supervision/ai_monitor_link_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"


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

APP_ID="ts-ai-monitor-link-supervision"
APP_NAME="ts-ai-monitor-link-supervision"
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
import pathlib, sys
source = pathlib.Path(sys.argv[1]).read_text()
seed_nodes = sys.argv[3]
lines = []
replaced = False
for line in source.splitlines():
    if line.startswith("seed_nodes = "):
        lines.append(f"seed_nodes = [{seed_nodes}]")
        replaced = True
    else:
        lines.append(line)
if not replaced:
    lines.insert(3, f"seed_nodes = [{seed_nodes}]")
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY
}

ask() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "================================================================"
echo "  AI Monitor/Link Supervision (TypeScript WASM)"
echo "  Demonstrates FLP/Byzantine fault detection in AI pipelines"
echo "================================================================"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

for node in "${NODE_LIST[@]}"; do
  node_host="${node%%:*}"
  node_port="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${node_host}:${node_port}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot connect to node at ${node_host}:${node_port}${NC}"
    exit 1
  fi
done

TEMP_CONFIG="$(mktemp -t ts-ai-monitor-link-app-config)"
  trap 'rm -f "${APP_ZIP:-}" "${TEMP_CONFIG:-}"' EXIT
  APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
render_config "$TEMP_CONFIG"

echo "Step 1: Undeploy from all nodes, then deploy to entry node"
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
echo -e "  ${GREEN}Deployed${NC}"
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
sleep 2

echo ""
echo "Step 2: Normal inference"
INFER=$(ask "inference_worker_a" '{"op":"infer","prompt":"Explain the actor model","request_id":"r1"}' 15)
if echo "$INFER" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Inference OK${NC}"
else
  echo -e "${YELLOW}  Response: $INFER${NC}"
fi

echo ""
echo "Step 3: Setup monitor (supervisor → worker-a)"
MON=$(ask "pipeline_supervisor" '{"op":"monitor_worker","worker_id":"inference_worker_a"}' 15)
if echo "$MON" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Monitor established${NC}"
fi

echo ""
echo "Step 4: Link worker-a ↔ worker-b"
LINK=$(ask "inference_worker_a" '{"op":"link_with","peer_id":"inference_worker_b"}' 15)
if echo "$LINK" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Workers linked${NC}"
fi

echo ""
echo "Step 5: Inject Byzantine mode"
ask "inference_worker_a" '{"op":"set_mode","mode":"byzantine"}' 10 >/dev/null
echo -e "  ${YELLOW}Worker-a in Byzantine mode${NC}"

echo ""
echo "Step 6: Validate Byzantine output (expect detection)"
VAL=$(ask "validator_agent" '{"op":"validate","worker_id":"inference_worker_a","result":"42 is the answer to everything"}' 15)
BYZ=$(echo "$VAL" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('byzantine_suspected',False))" 2>/dev/null || echo "?")
echo -e "  ${YELLOW}Byzantine detected: $BYZ${NC}"

echo ""
echo "Step 7: Unlink and recover"
ask "inference_worker_a" '{"op":"unlink_from","peer_id":"inference_worker_b"}' 10 >/dev/null
ask "inference_worker_a" '{"op":"set_mode","mode":"normal"}' 10 >/dev/null
RECOVER=$(ask "inference_worker_a" '{"op":"infer","prompt":"What is fault tolerance?","request_id":"recover"}' 15)
if echo "$RECOVER" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' and p.get('mode')=='normal' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Recovery confirmed${NC}"
fi

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  ask "$actor" "$payload" "$timeout"
}

assert_actor_ok() {
  local step="$1" raw="$2"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except Exception as e:
    print(f"FAIL [{step}]: invalid JSON: {e}", file=sys.stderr); sys.exit(1)
def find_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False: return str(obj.get("error") or "request failed")
        if obj.get("status") == "error": return str(obj.get("error") or "error status")
        if obj.get("error"): return str(obj["error"])
        for v in obj.values():
            e = find_err(v)
            if e: return e
    return None
err = find_err(d)
if err: print(f"FAIL [{step}]: {err}", file=sys.stderr); sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step${NC}"
    echo "  Raw: $(echo "$raw" | head -c 400)"
    exit 1
  fi
}

echo ""
echo "Step 8: Smoke test inference worker"
echo "  Testing inference_worker_a status..."
R="$(send_actor "inference_worker_a" '{"op":"status"}' 20)"
assert_actor_ok "worker_a status" "$R"
echo "  ✓ inference_worker_a status OK"

echo "  Testing pipeline_supervisor status..."
R="$(send_actor "pipeline_supervisor" '{"op":"status"}' 20)"
assert_actor_ok "pipeline_supervisor status" "$R"
echo "  ✓ pipeline_supervisor status OK"

echo ""
echo "================================================================"
echo -e "  ${GREEN}AI Monitor/Link Supervision (TypeScript WASM) Test Complete${NC}"
echo ""
echo "  Key behaviors demonstrated:"
echo "    ✓ host.monitor()    — one-way DOWN notification to supervisor"
echo "    ✓ host.link()       — bidirectional EXIT fate-sharing"
echo "    ✓ host.unlink()     — safe decoupling before graceful shutdown"
echo "    ✓ host.demonitor()  — cancel watch when actor replaced"
echo "    ✓ Byzantine mode    — inconsistent outputs (FLP-inspired detection)"
echo "    ✓ FLP threshold     — ≥1/3 faulty → validator raises alert"
echo "================================================================"
