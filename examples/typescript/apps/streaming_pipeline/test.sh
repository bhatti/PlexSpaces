#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/streaming_actor.wasm"
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
  NODES=""
  for _port in "$@"; do
    NODES="${NODES:+$NODES }localhost:$_port"
  done
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="streaming-pipeline-ts"
APP_NAME="streaming-pipeline-ts"
LEADER_ACTOR="streaming-leader"
WORKER_COUNT=8
BATCH_COUNT=18
EVENTS_PER_BATCH=1200
DROP_RATE=0.08
ENRICH_FIELDS=6

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"
TEMP_DIR=""
ROUTING_SOURCE="$REPO_ROOT/crates/actor/src/routing.rs"

current_binary_mtime_epoch() {
  (cd "$REPO_ROOT" && python3 - <<'PY'
import os
print(int(os.path.getmtime("target/debug/plexspaces")))
PY
)
}

assert_binary_newer_than_sources() {
  (cd "$REPO_ROOT" && python3 - "$ROUTING_SOURCE" <<'PY'
import os
import sys

binary = "target/debug/plexspaces"
sources = sys.argv[1:]
if not os.path.exists(binary):
    print(f"missing binary: {binary}")
    raise SystemExit(1)
binary_mtime = os.path.getmtime(binary)
newer = [path for path in sources if os.path.exists(path) and os.path.getmtime(path) > binary_mtime]
if newer:
    print("server binary is older than source changes:")
    for path in newer:
        print(path)
    raise SystemExit(1)
PY
)
}

assert_node_binaries_current() {
"$SCRIPT_DIR/undeploy.sh" $NODES
}


seed_nodes_list() {
  local joined="" sep=""
  for node in "${NODE_LIST[@]}"; do
    joined="${joined}${sep}\"${node}\""
    sep=", "
  done
  printf '%s' "$joined"
}

render_config() {
  local temp_config="$1"
  local seeds
  seeds="$(seed_nodes_list)"
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

list_registered_node_count() {
  local host="$1" port="$2"
  local raw
  raw="$(curl -s --connect-timeout 5 --max-time 15 ${AUTH_HEADER:+-H "$AUTH_HEADER"} "http://${host}:${port}/api/v1/nodes?page_size=100" 2>/dev/null || true)"
  RAW_RESPONSE="$raw" python3 - <<'PY'
import json
import os

raw = os.environ.get("RAW_RESPONSE", "").strip()
if not raw:
    print(0)
    raise SystemExit(0)
try:
    payload = json.loads(raw)
except Exception:
    print(0)
    raise SystemExit(0)
nodes = payload.get("nodes", [])
if not isinstance(nodes, list):
    print(0)
    raise SystemExit(0)
# Only count active nodes (status=2); disconnected/unknown nodes (status=3) must not count
active = [n for n in nodes if n.get("status") == 2]
print(len(active))
PY
}

wait_for_registry_membership() {
  local expected="$1"
  local attempts="${2:-30}"
  local sleep_s="${3:-2}"
  local ready=false

  for attempt in $(seq 1 "$attempts"); do
    ready=true
    for node in "${NODE_LIST[@]}"; do
      local host="${node%%:*}"
      local port="${node##*:}"
      local count
      count="$(list_registered_node_count "$host" "$port")"
      if [[ "$count" -lt "$expected" ]]; then
        ready=false
        echo -e "  ${YELLOW}Registry on ${host}:${port} sees ${count}/${expected} nodes (attempt ${attempt}/${attempts})...${NC}"
        break
      fi
    done
    if [[ "$ready" == "true" ]]; then
      return 0
    fi
    sleep "$sleep_s"
  done

  echo -e "${RED}Node registry did not converge to ${expected} nodes before the run${NC}"
  for node in "${NODE_LIST[@]}"; do
    local host="${node%%:*}"
    local port="${node##*:}"
    echo "  Registered nodes from http://${host}:${port}/api/v1/nodes"
    curl -s --connect-timeout 5 --max-time 30 ${AUTH_HEADER:+-H "$AUTH_HEADER"} "http://${host}:${port}/api/v1/nodes?page_size=100" | sed 's/^/    /' || echo "    (request failed)"
  done
  exit 1
}

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

assert_binary_newer_than_sources

for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${host}:${port}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot reach node at ${host}:${port}${NC}"
    exit 1
  fi
done

TEMP_DIR="$(mktemp -d)"
TEMP_CONFIG="$TEMP_DIR/app-config.toml"
  trap 'rm -rf "${TEMP_DIR:-}" "${APP_ZIP:-}"' EXIT
  APP_ZIP="$(mktemp).zip"
rm -f "$APP_ZIP"
render_config "$TEMP_CONFIG"
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null

echo "Step 1: Undeploy existing app from all nodes"
"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2

echo "Step 1: Deploy to all nodes (non-entry first, entry last)"
for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then
    continue
  fi
  host="${node%%:*}"
  port="${node##*:}"
  _deployed=0
  for _attempt in 1 2 3; do
    deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${host}:${port}/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "app_file=@$APP_ZIP" 2>&1)
    http_code=$(echo "$deploy_output" | tail -n1)
    response=$(echo "$deploy_output" | sed '$d')
    if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
      _deployed=1
      break
    fi
    echo "  Deploy attempt $_attempt to ${host}:${port} failed, retrying in 3s..."
    sleep 3
  done
  if [ "$_deployed" -eq 0 ]; then
    echo -e "${RED}Deploy to ${host}:${port} failed: $response${NC}"
    exit 1
  fi
done
_deployed=0
for _attempt in 1 2 3; do
  APP_ZIP="$(mktemp).zip"
rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1)
  http_code=$(echo "$deploy_output" | tail -n1)
  response=$(echo "$deploy_output" | sed '$d')
  if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $response${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed${NC}"
sleep 2
rm -rf "$TEMP_DIR"
TEMP_DIR=""

echo "Step 1a: Wait for registry convergence"
wait_for_registry_membership "${#NODE_LIST[@]}" "${STREAMING_PIPELINE_REGISTRY_ATTEMPTS:-5}" "${STREAMING_PIPELINE_REGISTRY_SLEEP_SECS:-2}"

echo "Step 1b: ListConnectedNodes (GET /api/v1/nodes) per node"
for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  echo "  Registered nodes from http://${host}:${port}/api/v1/nodes"
  curl -s --connect-timeout 5 --max-time 30 ${AUTH_HEADER:+-H "$AUTH_HEADER"} "http://${host}:${port}/api/v1/nodes?page_size=100" | sed 's/^/    /' || echo "    (request failed)"
done
echo ""

BOLD='\033[1m'
SCALING_SHARDS="${SCALING_SHARDS:-2,4,8,16}"
require_nonempty() { if [[ -z "$2" ]]; then echo -e "${RED}FAIL [$1]${NC}"; exit 1; fi; }
ask_actor() {
  curl -s --max-time "$2" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$1/ask?timeout=$2" \
    -H "Content-Type: application/json" ${AUTH_HEADER:+-H "$AUTH_HEADER"} -d "$3" 2>/dev/null || echo '{"error":"timeout"}'
}

echo -e "\n${BOLD}Step 2: Basic streaming run (18 batches × 1200 events, 8 workers)${NC}"
run_payload='{"op":"run","worker_count":8,"batch_count":18,"events_per_batch":1200,"drop_rate":0.08,"enrich_fields":6}'
BASIC=""
for _a in $(seq 1 10); do
  BASIC=$(ask_actor "$LEADER_ACTOR" 120 "$run_payload")
  if echo "$BASIC" | grep -q '"status":"ok"'; then
    # Check multi-node participation
    got_nodes=$(echo "$BASIC" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('node_count',0))" 2>/dev/null || echo "0")
    if [[ "$got_nodes" -ge "${#NODE_LIST[@]}" ]]; then break; fi
  fi
  sleep 3
done
require_nonempty "basic" "$BASIC"

python3 - "$BASIC" "${#NODE_LIST[@]}" <<'PY'
import json, sys
r = json.loads(sys.argv[1])
r = r.get("payload", r)
if isinstance(r, str): r = json.loads(r)
expected_nodes = int(sys.argv[2])
assert r.get("status") == "ok", f"status={r.get('status')}"
w = r.get("wall_time_ms",0); c = r.get("compute_time_ms",0); co = r.get("coordination_time_ms",0)
total = c+co or 1
print("═" * 80); print("  Streaming Pipeline (TypeScript) — Basic Run"); print("═" * 80)
print(f"  Events={r.get('event_count',0)}, workers={r.get('worker_count',0)}, batches={r.get('batch_count',0)}, rounds={r.get('stream_rounds',0)}")
print(f"  Timing: wall={w}ms, compute={c}ms ({c*100//total}%), coord={co}ms ({co*100//total}%), gran={r.get('granularity_ratio',0):.2f}x")
print(f"  Throughput: {r.get('events_per_sec',0)} events/s, {r.get('bytes_processed',0)} bytes")
print(f"  Pipeline: filtered={r.get('filtered_event_count',0)}, enriched={r.get('enriched_event_count',0)}, transformed={r.get('transformed_event_count',0)}, dropped={r.get('dropped_event_count',0)}")
print(f"  Nodes={r.get('node_count',0)}, workers_on_remote={r.get('worker_node_count',0)}, errors={r.get('error_count',0)}")
print("═" * 80)
assert r.get("event_count",0) > 0; assert r.get("error_count",0) == 0
assert r.get("node_count",0) >= expected_nodes, f"expected {expected_nodes} nodes, got {r.get('node_count',0)}"
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 2 passed${NC}"

echo -e "\n${BOLD}Step 3: Strong scaling${NC}"
SP=$(python3 -c "import json; print(json.dumps({'op':'run_scaling_benchmark','events_per_batch':1200,'batch_count':18,'shard_counts':[int(s) for s in '${SCALING_SHARDS}'.split(',')],'benchmark_rounds':1}))")
SR=$(ask_actor "$LEADER_ACTOR" 600 "$SP"); require_nonempty "scaling" "$SR"

python3 - "$SR" <<'PY'
import json, sys
r = json.loads(sys.argv[1])
r = r.get("payload", r)
if isinstance(r, str): r = json.loads(r)
assert r.get("status") == "ok"
print("═" * 90); print("  Streaming Pipeline (TypeScript) — Strong Scaling"); print("═" * 90)
print(f"  {'Shards':>8} {'Evt/s':>8} {'Wall':>8} {'Comp':>8} {'Coord':>9} {'Gran':>6} {'Speed':>8} {'Eff%':>6} {'Errs':>5}")
print("─" * 90)
for row in r.get("results",[]):
    print(f"  {row.get('shards',0):>8} {row.get('events_per_sec',0):>8} {row.get('wall_time_ms',0):>8} {row.get('compute_time_ms',0):>8} {row.get('coordination_time_ms',0):>9} {row.get('granularity_ratio',0):>6.1f} {row.get('speedup',0):>7.2f}x {row.get('efficiency_pct',0):>5.1f}% {row.get('error_count',0):>5}")
print("═" * 90)
for row in r.get("results",[]): assert row.get("error_count",0) == 0
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 3 passed${NC}"

echo -e "\n${BOLD}Step 4: Weak scaling${NC}"
WP=$(python3 -c "import json; print(json.dumps({'op':'run_weak_scaling_benchmark','events_per_worker':1200,'shard_counts':[int(s) for s in '${SCALING_SHARDS}'.split(',')],'num_passes':4,'benchmark_rounds':1}))")
WR=$(ask_actor "$LEADER_ACTOR" 300 "$WP"); require_nonempty "weak_scaling" "$WR"

python3 - "$WR" <<'PY'
import json, sys
r = json.loads(sys.argv[1])
r = r.get("payload", r)
if isinstance(r, str): r = json.loads(r)
assert r.get("status") == "ok"
print("═" * 88); print("  Streaming Pipeline (TypeScript) — Weak Scaling (1200 events/worker × 4 passes)"); print("═" * 88)
print(f"  {'Shards':>8} {'TotEvt':>8} {'Evt/s':>8} {'Wall':>8} {'Comp':>8} {'Coord':>9} {'Gran':>6} {'Eff%':>6} {'Errs':>5}")
print("─" * 88)
for row in r.get("results",[]):
    print(f"  {row.get('shards',0):>8} {row.get('total_events',0):>8} {row.get('events_per_sec',0):>8} {row.get('wall_time_ms',0):>8} {row.get('compute_time_ms',0):>8} {row.get('coordination_time_ms',0):>9} {row.get('granularity_ratio',0):>6.1f} {row.get('efficiency_pct',0):>5.1f}% {row.get('error_count',0):>5}")
print("═" * 88)
for row in r.get("results",[]): assert row.get("error_count",0) == 0
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 4 passed${NC}"

echo ""
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}  Streaming Pipeline (TypeScript) — All steps passed${NC}"
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
