#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_TS_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_TS_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/log_pipeline_actor.wasm"
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
  NODES=""
  for _port in "$@"; do
    NODES="${NODES:+$NODES }localhost:$_port"
  done
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="ts-log-pipeline"
APP_NAME="ts-log-pipeline"
TEMP_DIR=""

SCALING_SHARDS="${SCALING_SHARDS:-2,4,8,16}"
SCALING_EVENT_COUNT="${SCALING_EVENT_COUNT:-10000}"
SCALING_BATCH_SIZE="${SCALING_BATCH_SIZE:-500}"
SCALING_PIPELINE_DEPTH="${SCALING_PIPELINE_DEPTH:-5}"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"
LEADER_ACTOR_PATH="default:leader"

grpc_seed_nodes() {
  local joined=""
  local sep=""
  for entry in "${NODE_LIST[@]}"; do
    joined="${joined}${sep}\"${entry}\""
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

require_nonempty() {
  local label="$1"
  local response="$2"
  if [[ -z "$response" ]]; then
    echo -e "${RED}FAIL [$label]: empty response${NC}"
    exit 1
  fi
}

ask_actor() {
  local actor_path="$1"
  local timeout="$2"
  local payload="$3"
  curl -s --max-time "$timeout" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$actor_path/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# ═══════════════════════════════════════════════════════════════════════════════
#  BUILD
# ═══════════════════════════════════════════════════════════════════════════════

if [ ! -f "$WASM_FILE" ] || find "$SCRIPT_DIR" -maxdepth 3 \( -name '*.ts' -not -name '*.test.*' \) -newer "$WASM_FILE" -print -quit 2>/dev/null | grep -q .; then
  "$SCRIPT_DIR/build.sh"
fi

# ═══════════════════════════════════════════════════════════════════════════════
#  CONNECTIVITY CHECK
# ═══════════════════════════════════════════════════════════════════════════════

for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"
  p="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot connect to node at ${h}:${p}${NC}"
    exit 1
  fi
done

# ═══════════════════════════════════════════════════════════════════════════════
#  DEPLOY
# ═══════════════════════════════════════════════════════════════════════════════

echo -e "${BOLD}Step 1: Deploy${NC}"

TEMP_DIR="$(mktemp -d)"
TEMP_CONFIG="$TEMP_DIR/app-config.toml"
trap 'rm -rf "${TEMP_DIR:-}" "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp).zip"
rm -f "$APP_ZIP"
render_config "$TEMP_CONFIG"
zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null

"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2

for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then continue; fi
  host="${node%%:*}"
  port="${node##*:}"
  _deployed=0
  for _attempt in 1 2 3; do
    response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
      "http://${host}:${port}/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "app_file=@$APP_ZIP" 2>&1) || true
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')
    if [ "$http_code" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
      _deployed=1; break
    fi
    echo "  Deploy attempt $_attempt to ${host}:${port} failed, retrying in 3s..."
    sleep 3
  done
  if [ "$_deployed" -eq 0 ]; then
    echo -e "${RED}Deploy to ${host}:${port} failed: $body${NC}"; exit 1
  fi
done

_deployed=0
for _attempt in 1 2 3; do
  APP_ZIP="$(mktemp).zip"
  rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
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
    _deployed=1; break
  fi
  echo "  Deploy attempt $_attempt to ${ENTRY_HOST}:${ENTRY_PORT} failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy to ${ENTRY_HOST}:${ENTRY_PORT} failed: $body${NC}"; exit 1
fi
echo -e "${GREEN}Deployed to all nodes${NC}"

if [ "${#NODE_LIST[@]}" -gt 1 ]; then
  wait_for_registry_membership "${#NODE_LIST[@]}" 10 3
fi

# ═══════════════════════════════════════════════════════════════════════════════
#  Step 2: Basic Pipeline Run
# ═══════════════════════════════════════════════════════════════════════════════

echo -e "\n${BOLD}Step 2: Basic pipeline run (5000 events, 8 workers, depth 5)${NC}"

BASIC_RESPONSE=""
for _attempt in $(seq 1 10); do
  BASIC_RESPONSE=$(ask_actor "$LEADER_ACTOR_PATH" 120 '{
    "op": "run",
    "event_count": 5000,
    "worker_count": 8,
    "batch_size": 500,
    "pipeline_depth": 5,
    "sink_format": "splunk_hec",
    "rounds": 1
  }')
  if echo "$BASIC_RESPONSE" | grep -q '"status"[[:space:]]*:[[:space:]]*"ok"'; then
    break
  fi
  echo "  Attempt $_attempt: placement not ready, retrying in 5s..."
  sleep 5
done

require_nonempty "basic_run" "$BASIC_RESPONSE"

python3 - "$BASIC_RESPONSE" <<'METRICS_PY'
import json
import sys

raw = sys.argv[1]
try:
    r = json.loads(raw)
except Exception as e:
    print(f"ERROR: failed to parse response: {e}")
    print(f"  Raw: {raw[:500]}")
    sys.exit(1)

r = r.get("payload", r)
if isinstance(r, str):
    r = json.loads(r)

if r.get("status") != "ok":
    print(f"ERROR: status={r.get('status', 'unknown')}")
    sys.exit(1)

print("═" * 72)
print("  Log Pipeline (TypeScript) — Basic Run Results")
print("═" * 72)
print(f"  Data size:     events={r.get('event_count', 0)}, workers={r.get('worker_count', 0)}, depth={r.get('pipeline_depth', 0)}")
print(f"  Pipeline:      {r.get('pipeline_functions', '')}")
print(f"  Sink:          {r.get('sink_format', '')}")
print(f"  Topology:      node_count={r.get('node_count', 0)}, actor_count={r.get('actor_count', 0)}")

wall = r.get("wall_time_ms", 0)
comp = r.get("compute_time_ms", 0)
coord = r.get("coordination_time_ms", 0)
total = comp + coord if comp + coord > 0 else 1
print(f"  Timing:        wall_time_ms={wall}, compute_time_ms={comp} ({comp * 100 // total}%), coordination_time_ms={coord} ({coord * 100 // total}%)")
print(f"  Granularity:   ratio={r.get('granularity_ratio', 0)} (target ≥10x)")
print(f"  Throughput:    {r.get('events_per_sec', 0)} events/sec")
print(f"  Events:        in={r.get('total_events_in', 0)}, out={r.get('total_events_out', 0)}, dropped={r.get('total_events_dropped', 0)}")
print(f"  Worker lat:    avg={r.get('avg_worker_latency_ms', 0)}ms, max={r.get('max_worker_latency_ms', 0)}ms")
print(f"  Routes:        {json.dumps(r.get('route_distribution', {}))}")
print(f"  Errors:        {r.get('error_count', 0)}")
print("═" * 72)

assert r.get("total_events_out", 0) > 0, "events_out must be > 0"
assert r.get("error_count", 0) == 0, f"error_count must be 0, got {r.get('error_count')}"
assert r.get("events_per_sec", 0) > 0, "events_per_sec must be > 0"
print("  ✓ Basic run assertions passed")
METRICS_PY

echo -e "${GREEN}Step 2 passed${NC}"

# ═══════════════════════════════════════════════════════════════════════════════
#  Step 3: Strong Scaling Benchmark
# ═══════════════════════════════════════════════════════════════════════════════

echo -e "\n${BOLD}Step 3: Strong scaling benchmark (fixed ${SCALING_EVENT_COUNT} events, vary workers)${NC}"

IFS=',' read -ra SHARD_LIST <<< "$SCALING_SHARDS"
SCALING_PAYLOAD=$(python3 -c "
import json
shards = [int(s) for s in '${SCALING_SHARDS}'.split(',')]
print(json.dumps({
    'op': 'run_scaling_benchmark',
    'event_count': int('${SCALING_EVENT_COUNT}'),
    'shard_counts': shards,
    'batch_size': int('${SCALING_BATCH_SIZE}'),
    'pipeline_depth': int('${SCALING_PIPELINE_DEPTH}'),
    'warmup_rounds': 1,
    'benchmark_rounds': 2
}))
")

SCALING_RESPONSE=$(ask_actor "$LEADER_ACTOR_PATH" 300 "$SCALING_PAYLOAD")
require_nonempty "strong_scaling" "$SCALING_RESPONSE"

python3 - "$SCALING_RESPONSE" <<'SCALING_PY'
import json
import sys

raw = sys.argv[1]
try:
    r = json.loads(raw)
except Exception as e:
    print(f"ERROR: failed to parse: {e}")
    sys.exit(1)

r = r.get("payload", r)
if isinstance(r, str):
    r = json.loads(r)

if r.get("status") != "ok":
    print(f"ERROR: status={r.get('status')}")
    sys.exit(1)

results = r.get("results", [])
if not results:
    print("ERROR: no scaling results")
    sys.exit(1)

print("═" * 90)
print("  Log Pipeline (TypeScript) — Strong Scaling Benchmark")
print(f"  Fixed work: {r.get('event_count', 0)} events, depth={r.get('pipeline_depth', 0)}")
print("═" * 90)
print(f"  {'Workers':>8} {'Evts/s':>8} {'Wall ms':>8} {'Comp ms':>8} {'Coord ms':>9} {'Comp%':>6} {'Gran':>6} {'Speedup':>8} {'Eff%':>6} {'Nodes':>6} {'Errs':>5}")
print("─" * 90)

for row in results:
    print(
        f"  {row.get('shards', 0):>8}"
        f" {row.get('events_per_sec', 0):>8}"
        f" {row.get('wall_time_ms', 0):>8}"
        f" {row.get('compute_time_ms', 0):>8}"
        f" {row.get('coordination_time_ms', 0):>9}"
        f" {row.get('compute_pct', 0):>5.1f}%"
        f" {row.get('granularity_ratio', 0):>6.1f}"
        f" {row.get('speedup', 0):>7.2f}x"
        f" {row.get('efficiency_pct', 0):>5.1f}%"
        f" {row.get('node_count', 0):>6}"
        f" {row.get('error_count', 0):>5}"
    )

print("═" * 90)

for row in results:
    assert row.get("error_count", 0) == 0, f"errors at {row.get('shards')} shards"
    assert row.get("events_per_sec", 0) > 0, f"zero throughput at {row.get('shards')} shards"

print("  ✓ Strong scaling assertions passed")
SCALING_PY

echo -e "${GREEN}Step 3 passed${NC}"

# ═══════════════════════════════════════════════════════════════════════════════
#  Step 4: Weak Scaling Benchmark
# ═══════════════════════════════════════════════════════════════════════════════

echo -e "\n${BOLD}Step 4: Weak scaling benchmark (5000 events/shard, grow total)${NC}"

WEAK_PAYLOAD=$(python3 -c "
import json
shards = [int(s) for s in '${SCALING_SHARDS}'.split(',')]
print(json.dumps({
    'op': 'run_weak_scaling_benchmark',
    'events_per_shard': 5000,
    'shard_counts': shards,
    'batch_size': int('${SCALING_BATCH_SIZE}'),
    'pipeline_depth': int('${SCALING_PIPELINE_DEPTH}'),
    'warmup_rounds': 1,
    'benchmark_rounds': 2
}))
")

WEAK_RESPONSE=$(ask_actor "$LEADER_ACTOR_PATH" 300 "$WEAK_PAYLOAD")
require_nonempty "weak_scaling" "$WEAK_RESPONSE"

python3 - "$WEAK_RESPONSE" <<'WEAK_PY'
import json
import sys

raw = sys.argv[1]
try:
    r = json.loads(raw)
except Exception as e:
    print(f"ERROR: failed to parse: {e}")
    sys.exit(1)

r = r.get("payload", r)
if isinstance(r, str):
    r = json.loads(r)

if r.get("status") != "ok":
    print(f"ERROR: status={r.get('status')}")
    sys.exit(1)

results = r.get("results", [])
print("═" * 80)
print("  Log Pipeline (TypeScript) — Weak Scaling Benchmark")
print(f"  Fixed work per shard: {r.get('events_per_shard', 0)} events, depth={r.get('pipeline_depth', 0)}")
print("═" * 80)
print(f"  {'Workers':>8} {'Total':>8} {'Evts/s':>8} {'Wall ms':>8} {'Comp ms':>8} {'Coord ms':>9} {'Gran':>6} {'Eff%':>6} {'Nodes':>6} {'Errs':>5}")
print("─" * 80)

for row in results:
    print(
        f"  {row.get('shards', 0):>8}"
        f" {row.get('total_events', 0):>8}"
        f" {row.get('events_per_sec', 0):>8}"
        f" {row.get('wall_time_ms', 0):>8}"
        f" {row.get('compute_time_ms', 0):>8}"
        f" {row.get('coordination_time_ms', 0):>9}"
        f" {row.get('granularity_ratio', 0):>6.1f}"
        f" {row.get('efficiency_pct', 0):>5.1f}%"
        f" {row.get('node_count', 0):>6}"
        f" {row.get('error_count', 0):>5}"
    )

print("═" * 80)

for row in results:
    assert row.get("error_count", 0) == 0, f"errors at {row.get('shards')} shards"

print("  ✓ Weak scaling assertions passed")
WEAK_PY

echo -e "${GREEN}Step 4 passed${NC}"

# ═══════════════════════════════════════════════════════════════════════════════
#  Step 5: Pipeline Depth Benchmark
# ═══════════════════════════════════════════════════════════════════════════════

echo -e "\n${BOLD}Step 5: Pipeline depth benchmark (vary function chain length)${NC}"

DEPTH_RESPONSE=$(ask_actor "$LEADER_ACTOR_PATH" 180 '{
  "op": "run_pipeline_depth_benchmark",
  "event_count": 10000,
  "worker_count": 8,
  "batch_size": 500,
  "depths": [1, 2, 3, 4, 5]
}')
require_nonempty "pipeline_depth" "$DEPTH_RESPONSE"

python3 - "$DEPTH_RESPONSE" <<'DEPTH_PY'
import json
import sys

raw = sys.argv[1]
try:
    r = json.loads(raw)
except Exception as e:
    print(f"ERROR: failed to parse: {e}")
    sys.exit(1)

r = r.get("payload", r)
if isinstance(r, str):
    r = json.loads(r)

results = r.get("results", [])
print("═" * 80)
print("  Log Pipeline (TypeScript) — Pipeline Depth Benchmark")
print(f"  Fixed: {r.get('event_count', 0)} events, {r.get('worker_count', 0)} workers")
print("═" * 80)
print(f"  {'Depth':>6} {'Functions':>40} {'Evts/s':>8} {'Wall ms':>8} {'Gran':>6}")
print("─" * 80)

for row in results:
    print(
        f"  {row.get('depth', 0):>6}"
        f" {row.get('functions', ''):>40}"
        f" {row.get('events_per_sec', 0):>8}"
        f" {row.get('wall_time_ms', 0):>8}"
        f" {row.get('granularity_ratio', 0):>6.1f}"
    )

print("═" * 80)

for row in results:
    assert row.get("events_per_sec", 0) > 0, f"zero throughput at depth {row.get('depth')}"

print("  ✓ Pipeline depth assertions passed")
DEPTH_PY

echo -e "${GREEN}Step 5 passed${NC}"

# ═══════════════════════════════════════════════════════════════════════════════
#  Step 6: Application Metrics
# ═══════════════════════════════════════════════════════════════════════════════

echo -e "\n${BOLD}Step 6: Application metrics check${NC}"

APP_STATUS=$(curl -s --max-time 10 \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/${APP_ID}/status" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} 2>/dev/null || echo '{}')

python3 - "$APP_STATUS" <<'STATUS_PY'
import json
import sys

raw = sys.argv[1]
try:
    r = json.loads(raw)
except Exception:
    print("WARNING: could not parse application status")
    sys.exit(0)

print(f"  Application: {r.get('application_id', 'unknown')}")
print(f"  Status:      {r.get('status', 'unknown')}")
metrics = r.get("metrics", {})
if metrics:
    msg_count = metrics.get("message_count", 0)
    counters = metrics.get("counter_metrics", {})
    latencies = metrics.get("latency_totals_ms", {})
    print(f"  Messages:    {msg_count}")
    if counters:
        for k, v in sorted(counters.items())[:10]:
            print(f"    {k}: {v}")
    if latencies:
        for k, v in sorted(latencies.items())[:10]:
            print(f"    {k}: {v}ms")
STATUS_PY

echo -e "${GREEN}Step 6 passed${NC}"

# ═══════════════════════════════════════════════════════════════════════════════
#  SUMMARY
# ═══════════════════════════════════════════════════════════════════════════════

echo ""
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}  Log Pipeline (TypeScript) — All steps passed${NC}"
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
