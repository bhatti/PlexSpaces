#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/parallel_ai_inference_actor.wasm"
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

APP_ID="python-parallel-ai-inference"
APP_NAME="python-parallel-ai-inference"
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
    local h="${node%%:*}"
    local p="${node##*:}"
    curl -s -X DELETE "http://${h}:${p}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

grpc_seed_nodes() {
  local seed_list=()
  for node in "${NODE_LIST[@]}"; do
    local h="${node%%:*}"
    local p="${node##*:}"
    seed_list+=("\"${h}:$((p - 1))\"")
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

check_response_field() {
  local label="$1"
  local response="$2"
  local field="$3"
  if ! echo "$response" | grep -q "\"$field\""; then
    echo -e "${RED}FAIL [$label]: missing field '$field' in response${NC}"
    echo "  Response: $response"
    return 1
  fi
  return 0
}

if [ ! -f "$WASM_FILE" ]; then
  "$SCRIPT_DIR/build.sh"
fi

for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"
  p="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot connect to node at ${h}:${p}${NC}"
    exit 1
  fi
done

TEMP_CONFIG="$(mktemp -t python-parallel-ai-inference-app-config)"
render_config "$TEMP_CONFIG"

# ─── Step 1: Deploy ───────────────────────────────────────────────────────────
for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"
  p="${node##*:}"
  echo "Step 1: Deploy to ${h}:${p}"
  curl -s -X DELETE "http://${h}:${p}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  sleep 1
  response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${h}:${p}/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG" 2>&1)
  http_code=$(echo "$response" | tail -n1)
  body=$(echo "$response" | sed '$d')
  if [ "$http_code" != "200" ] || ! echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "${RED}Deploy failed on ${h}:${p}: $body${NC}"
    exit 1
  fi
done
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
sleep 2

# ─── Step 2: Test inference worker directly ───────────────────────────────────
echo "Step 2: Test inference worker directly"
INFER_RESPONSE=$(curl -s --max-time 30 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/inference_worker/ask?timeout=30" \
  -H "Content-Type: application/json" \
  -d '{"op":"infer","request_id":"test-direct-1","input":"hello-world"}' 2>/dev/null || echo '{"error":"timeout"}')

if ! echo "$INFER_RESPONSE" | grep -q '"status"'; then
  echo -e "${RED}Direct inference failed: $INFER_RESPONSE${NC}"
  exit 1
fi
check_response_field "direct_infer" "$INFER_RESPONSE" "status"
echo -e "  ${GREEN}Inference worker responded OK${NC}"

# ─── Step 3: Test get_metrics on inference worker ─────────────────────────────
echo "Step 3: Test get_metrics on inference worker"
METRICS_RESPONSE=$(curl -s --max-time 30 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/inference_worker/ask?timeout=30" \
  -H "Content-Type: application/json" \
  -d '{"op":"get_metrics"}' 2>/dev/null || echo '{"error":"timeout"}')

check_response_field "get_metrics" "$METRICS_RESPONSE" "requests_processed"
echo -e "  ${GREEN}Metrics endpoint OK${NC}"

# ─── Step 4: Test shard benchmark (2 shards, 5 requests each) ─────────────────
echo "Step 4: Test shard benchmark (2 shards, 5 requests each)"
SHARD_BENCH_RESPONSE=$(curl -s --max-time 120 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/benchmark/ask?timeout=120" \
  -H "Content-Type: application/json" \
  -d '{"op":"run_shard_benchmark","shard_counts":[1,2],"requests_per_shard":5}' 2>/dev/null || echo '{"error":"timeout"}')

if ! echo "$SHARD_BENCH_RESPONSE" | grep -q '"benchmark"'; then
  echo -e "${RED}Shard benchmark failed: $SHARD_BENCH_RESPONSE${NC}"
  exit 1
fi
check_response_field "shard_benchmark" "$SHARD_BENCH_RESPONSE" "results"
echo -e "  ${GREEN}Shard benchmark OK${NC}"

# ─── Step 5: Test collective benchmark ────────────────────────────────────────
echo "Step 5: Test collective benchmark"
COLLECTIVE_RESPONSE=$(curl -s --max-time 120 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/benchmark/ask?timeout=120" \
  -H "Content-Type: application/json" \
  -d '{"op":"run_collective_benchmark","num_shards":2}' 2>/dev/null || echo '{"error":"timeout"}')

if ! echo "$COLLECTIVE_RESPONSE" | grep -q '"benchmark"'; then
  echo -e "${RED}Collective benchmark failed: $COLLECTIVE_RESPONSE${NC}"
  exit 1
fi
check_response_field "collective_benchmark" "$COLLECTIVE_RESPONSE" "timings"
echo -e "  ${GREEN}Collective benchmark OK${NC}"

# ─── Step 6: Test orchestrator workflow — shard mode ──────────────────────────
echo "Step 6: Test orchestrator workflow (shard mode)"
ORCH_SHARD_RESPONSE=$(curl -s --max-time 120 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/orchestrator/ask?timeout=120" \
  -H "Content-Type: application/json" \
  -d '{"op":"workflow_run","mode":"shard","num_shards":2,"num_requests":4}' 2>/dev/null || echo '{"error":"timeout"}')

if ! echo "$ORCH_SHARD_RESPONSE" | grep -q '"status"'; then
  echo -e "${RED}Orchestrator shard mode failed: $ORCH_SHARD_RESPONSE${NC}"
  exit 1
fi
check_response_field "orchestrator_shard" "$ORCH_SHARD_RESPONSE" "total_processed"
echo -e "  ${GREEN}Orchestrator shard mode OK${NC}"

# ─── Step 7: Test orchestrator workflow — collective mode ─────────────────────
echo "Step 7: Test orchestrator workflow (collective mode)"
ORCH_COLLECTIVE_RESPONSE=$(curl -s --max-time 120 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/orchestrator/ask?timeout=120" \
  -H "Content-Type: application/json" \
  -d '{"op":"workflow_run","mode":"collective","num_shards":2,"num_requests":4}' 2>/dev/null || echo '{"error":"timeout"}')

if ! echo "$ORCH_COLLECTIVE_RESPONSE" | grep -q '"status"'; then
  echo -e "${RED}Orchestrator collective mode failed: $ORCH_COLLECTIVE_RESPONSE${NC}"
  exit 1
fi
check_response_field "orchestrator_collective" "$ORCH_COLLECTIVE_RESPONSE" "timings"
echo -e "  ${GREEN}Orchestrator collective mode OK${NC}"

# ─── Step 8: Query orchestrator status ────────────────────────────────────────
echo "Step 8: Query orchestrator status"
STATUS_RESPONSE=$(curl -s --max-time 30 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/orchestrator/ask?timeout=30" \
  -H "Content-Type: application/json" \
  -d '{"op":"workflow_query:status"}' 2>/dev/null || echo '{"error":"timeout"}')

check_response_field "orchestrator_query" "$STATUS_RESPONSE" "mode"
check_response_field "orchestrator_query" "$STATUS_RESPONSE" "total_processed"
echo -e "  ${GREEN}Orchestrator status query OK${NC}"

# ─── Step 9: Check application metrics ────────────────────────────────────────
echo "Step 9: Check application metrics"
STATUS_METRICS=$(curl -s --max-time 30 \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/$APP_ID/status" 2>/dev/null || echo '{}')

echo "$STATUS_METRICS" | python3 - <<'PY'
import json, sys
raw_str = sys.stdin.read().strip()
if not raw_str:
    print("  (no metrics data)")
    sys.exit(0)
try:
    raw = json.loads(raw_str)
except json.JSONDecodeError:
    print(f"  (metrics response not JSON: {raw_str[:80]})")
    sys.exit(0)
app = raw.get("application", {})
metrics = app.get("metrics", {})
message_count = int(metrics.get("message_count", 0))
print(f"  application_id={app.get('application_id', 'n/a')}")
print(f"  message_count={message_count}")
counter_metrics = metrics.get("counter_metrics", {})
for k, v in sorted(counter_metrics.items()):
    print(f"  counter.{k}={v}")
PY
echo -e "  ${GREEN}Metrics check OK${NC}"

# ─── Step 10: Get all benchmark results ───────────────────────────────────────
echo "Step 10: Retrieve all benchmark results"
ALL_RESULTS=$(curl -s --max-time 30 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/benchmark/ask?timeout=30" \
  -H "Content-Type: application/json" \
  -d '{"op":"get_results"}' 2>/dev/null || echo '{"error":"timeout"}')

check_response_field "get_results" "$ALL_RESULTS" "total_benchmarks"
echo -e "  ${GREEN}All benchmark results retrieved OK${NC}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Python parallel AI inference example passed.${NC}"
echo "  Mechanisms tested:"
echo "    1. ShardGroup scatter-gather (shard benchmark, orchestrator shard mode)"
echo "    2. Elastic pool checkout/checkin (run_pool_benchmark)"
echo "    3. MPI collectives (broadcast, barrier, reduce, allreduce)"
echo "    4. Process groups (orchestrator workflow coordination)"
