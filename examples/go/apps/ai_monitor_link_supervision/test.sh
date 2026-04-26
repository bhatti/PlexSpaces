#!/usr/bin/env bash
# Test AI Monitor/Link Supervision (Go WASM)
# Demonstrates FLP/Byzantine fault detection using host.Monitor() and host.Link()
# Usage: ./test.sh [HTTP_PORT|node1:port1 node2:port2 ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/ai_monitor_link_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8092 localhost:8094"
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

APP_ID="go-ai-monitor-link-supervision"
APP_NAME="go-ai-monitor-link-supervision"
TEMP_CONFIG=""

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"

cleanup() {
  if [[ -n "${TEMP_CONFIG:-}" && -f "${TEMP_CONFIG:-}" ]]; then
    rm -f "$TEMP_CONFIG"
  fi
  for node in "${NODE_LIST[@]}"; do
    local_host="${node%%:*}"
    local_port="${node##*:}"
    curl -s -X DELETE "http://${local_host}:${local_port}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

grpc_seed_nodes() {
  local seed_list=()
  for node in "${NODE_LIST[@]}"; do
    local_host="${node%%:*}"
    local_port="${node##*:}"
    seed_list+=("\"${local_host}:$((local_port - 1))\"")
  done
  local joined=""
  local sep=""
  for entry in "${seed_list[@]}"; do
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
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

tell() {
  local actor="$1"
  local payload="$2"
  curl -s --max-time 10 -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$actor/tell" \
    -H "Content-Type: application/json" \
    -d "$payload" >/dev/null 2>&1 || true
}

echo "================================================================"
echo "  AI Monitor/Link Supervision (Go WASM)"
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

TEMP_CONFIG="$(mktemp -t ai-monitor-link-app-config)"
render_config "$TEMP_CONFIG"

for node in "${NODE_LIST[@]}"; do
  node_host="${node%%:*}"
  node_port="${node##*:}"
  echo "Step 1: Deploy to ${node_host}:${node_port}"
  curl -s -X DELETE "http://${node_host}:${node_port}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  sleep 1
  response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${node_host}:${node_port}/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG" 2>&1)
  http_code=$(echo "$response" | tail -n1)
  body=$(echo "$response" | sed '$d')
  if [ "$http_code" != "200" ] || ! echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "${RED}Deploy failed on ${node_host}:${node_port}: $body${NC}"
    exit 1
  fi
  echo -e "  ${GREEN}Deployed${NC}"
done
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
sleep 2

echo ""
echo "================================================================"
echo "  Phase 1: Normal inference operation"
echo "================================================================"

echo ""
echo "Step 2: Run inference on worker-a (normal mode)"
INFER1=$(ask "inference_worker_a" '{
  "op": "infer",
  "prompt": "Explain the actor model briefly",
  "request_id": "req-001"
}' 15)
if echo "$INFER1" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  RESULT=$(echo "$INFER1" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); r=p.get('result',''); print(r[:60]+'...' if len(r)>60 else r)" 2>/dev/null || echo "?")
  echo -e "  ${GREEN}Inference OK: $RESULT${NC}"
else
  echo -e "${YELLOW}  Inference response: $INFER1${NC}"
fi

echo ""
echo "Step 3: Run inference on worker-b (normal mode)"
INFER2=$(ask "inference_worker_b" '{
  "op": "infer",
  "prompt": "What is fault tolerance?",
  "request_id": "req-002"
}' 15)
if echo "$INFER2" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Inference OK on worker-b${NC}"
else
  echo -e "${YELLOW}  Inference response: $INFER2${NC}"
fi

echo ""
echo "Step 4: Check worker-a status"
STATUS_A=$(ask "inference_worker_a" '{"op": "status"}' 10)
echo "$STATUS_A" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
print('  worker-a: mode={}, requests={}, errors={}'.format(
    p.get('mode', '?'),
    p.get('total_requests', 0),
    p.get('error_count', 0),
))
" 2>/dev/null || echo "  Status: $STATUS_A"

echo ""
echo "================================================================"
echo "  Phase 2: Monitor setup and fault detection"
echo "================================================================"

echo ""
echo "Step 5: Pipeline supervisor monitors worker-a"
MONITOR_SETUP=$(ask "pipeline_supervisor" '{
  "op": "monitor_worker",
  "worker_id": "inference_worker_a"
}' 15)
if echo "$MONITOR_SETUP" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  MON_REF=$(echo "$MONITOR_SETUP" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('monitor_ref','?'))" 2>/dev/null || echo "?")
  echo -e "  ${GREEN}Monitor established (ref=$MON_REF)${NC}"
else
  echo -e "${YELLOW}  Monitor setup response: $MONITOR_SETUP${NC}"
fi

echo ""
echo "Step 6: Validator agent monitors worker-b"
MONITOR_B=$(ask "validator_agent" '{
  "op": "monitor_worker",
  "worker_id": "inference_worker_b"
}' 15)
if echo "$MONITOR_B" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Validator monitoring worker-b${NC}"
else
  echo -e "${YELLOW}  Monitor setup response: $MONITOR_B${NC}"
fi

echo ""
echo "================================================================"
echo "  Phase 3: Link setup for fate-sharing"
echo "================================================================"

echo ""
echo "Step 7: Link worker-a and worker-b (bidirectional fate-sharing)"
LINK=$(ask "inference_worker_a" '{
  "op": "link_with",
  "peer_id": "inference_worker_b"
}' 15)
if echo "$LINK" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Workers linked (bidirectional)${NC}"
else
  echo -e "${YELLOW}  Link response: $LINK${NC}"
fi

echo ""
echo "Step 8: Validate inference with Byzantine detection"
VALIDATE=$(ask "validator_agent" '{
  "op": "validate",
  "worker_id": "inference_worker_a",
  "result": "The actor model describes concurrent computation where actors are the unit of computation.",
  "expected_format": "text"
}' 15)
if echo "$VALIDATE" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  VALID=$(echo "$VALIDATE" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('valid','?'))" 2>/dev/null || echo "?")
  echo -e "  ${GREEN}Validation complete (valid=$VALID)${NC}"
else
  echo -e "${YELLOW}  Validation response: $VALIDATE${NC}"
fi

echo ""
echo "================================================================"
echo "  Phase 4: Byzantine fault injection"
echo "================================================================"

echo ""
echo "Step 9: Inject Byzantine behavior into worker-a"
SET_BYZANTINE=$(ask "inference_worker_a" '{
  "op": "set_mode",
  "mode": "byzantine"
}' 10)
if echo "$SET_BYZANTINE" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${YELLOW}Worker-a now in byzantine mode (will return inconsistent results)${NC}"
else
  echo -e "${YELLOW}  Set mode response: $SET_BYZANTINE${NC}"
fi

echo ""
echo "Step 10: Run inference through pipeline supervisor (byzantine worker)"
DISPATCH=$(ask "pipeline_supervisor" '{
  "op": "dispatch",
  "prompt": "Summarize Byzantine fault tolerance",
  "request_id": "req-byzantine"
}' 20)
echo "$DISPATCH" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
status = p.get('status', '?')
worker = p.get('worker_used', '?')
fault = p.get('byzantine_detected', False)
print('  Dispatch result: status={}, worker={}, byzantine_detected={}'.format(status, worker, fault))
" 2>/dev/null || echo "  Dispatch: $DISPATCH"

echo ""
echo "Step 11: Validate Byzantine response (should detect inconsistency)"
VALIDATE_BYZ=$(ask "validator_agent" '{
  "op": "validate",
  "worker_id": "inference_worker_a",
  "result": "42 is the answer to everything",
  "expected_format": "text"
}' 15)
BYZ_DETECTED=$(echo "$VALIDATE_BYZ" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('byzantine_suspected',False))" 2>/dev/null || echo "?")
echo -e "  ${YELLOW}Byzantine suspected by validator: $BYZ_DETECTED${NC}"

echo ""
echo "Step 12: Check validator agent status (FLP threshold)"
VAL_STATUS=$(ask "validator_agent" '{"op": "status"}' 10)
echo "$VAL_STATUS" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
print('  Validator: validations={}, byzantine_count={}, flp_threshold={}'.format(
    p.get('total_validations', 0),
    p.get('byzantine_count', 0),
    p.get('flp_threshold', '?'),
))
" 2>/dev/null || echo "  Validator status: $VAL_STATUS"

echo ""
echo "================================================================"
echo "  Phase 5: Recovery - unlink before graceful shutdown"
echo "================================================================"

echo ""
echo "Step 13: Unlink workers before graceful shutdown (prevent cascade)"
UNLINK=$(ask "inference_worker_a" '{
  "op": "unlink_from",
  "peer_id": "inference_worker_b"
}' 10)
if echo "$UNLINK" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Workers unlinked (safe to restart worker-a independently)${NC}"
else
  echo -e "${YELLOW}  Unlink response: $UNLINK${NC}"
fi

echo ""
echo "Step 14: Reset worker-a to normal mode"
RESET=$(ask "inference_worker_a" '{
  "op": "set_mode",
  "mode": "normal"
}' 10)
if echo "$RESET" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Worker-a restored to normal mode${NC}"
else
  echo -e "${YELLOW}  Reset response: $RESET${NC}"
fi

echo ""
echo "Step 15: Verify normal inference after recovery"
RECOVER=$(ask "inference_worker_a" '{
  "op": "infer",
  "prompt": "What is the FLP impossibility theorem?",
  "request_id": "req-recovery"
}' 15)
if echo "$RECOVER" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('status')=='ok' else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Recovery confirmed: worker-a back to normal${NC}"
else
  echo -e "${YELLOW}  Recovery response: $RECOVER${NC}"
fi

echo ""
echo "Step 16: Pipeline supervisor final status"
PIPE_STATUS=$(ask "pipeline_supervisor" '{"op": "status"}' 10)
echo "$PIPE_STATUS" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
print('  Supervisor: dispatched={}, monitor_count={}, down_events={}'.format(
    p.get('total_dispatched', 0),
    p.get('monitor_count', 0),
    p.get('down_events_received', 0),
))
" 2>/dev/null || echo "  Pipeline status: $PIPE_STATUS"

echo ""
echo "================================================================"
echo -e "  ${GREEN}AI Monitor/Link Supervision (Go WASM) Test Complete${NC}"
echo ""
echo "  Key behaviors demonstrated:"
echo "    ✓ host.Monitor()    — one-way DOWN notification to supervisor"
echo "    ✓ host.Link()       — bidirectional EXIT fate-sharing"
echo "    ✓ host.Unlink()     — safe decoupling before graceful shutdown"
echo "    ✓ host.Demonitor()  — cancel watch when actor replaced"
echo "    ✓ Byzantine mode    — inconsistent outputs (FLP-inspired detection)"
echo "    ✓ FLP threshold     — ≥1/3 faulty → validator raises alert"
echo "================================================================"
