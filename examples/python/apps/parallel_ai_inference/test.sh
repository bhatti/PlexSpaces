#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/parallel_ai_inference_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

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

APP_ID="python-parallel-ai-inference"
APP_NAME="python-parallel-ai-inference"
OTHER_APP_ID="rust-parallel-ai-inference"
TEMP_CONFIG=""
SCALING_SHARDS="${SCALING_SHARDS:-2,4,8,16,32}"
SCALING_REQUESTS_PER_SHARD="${SCALING_REQUESTS_PER_SHARD:-4}"
SCALING_WARMUP_REQUESTS="${SCALING_WARMUP_REQUESTS:-2}"
SCALING_LOGICAL_ACTORS="${SCALING_LOGICAL_ACTORS:-200}"
SCALING_DATA_SIZE_BYTES="${SCALING_DATA_SIZE_BYTES:-262144}"
SCALING_MODEL_TYPE="${SCALING_MODEL_TYPE:-large}"
SCALING_WORK_MULTIPLIER="${SCALING_WORK_MULTIPLIER:-10}"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"
WORKER_ACTOR_PATH="InferenceWorkerActor:default"
BENCHMARK_ACTOR_PATH="BenchmarkActor:default"
ORCHESTRATOR_ACTOR_PATH="OrchestratorWorkflow:default"


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

require_nonempty() {
  local label="$1"
  local response="$2"
  if [[ -z "$response" ]]; then
    echo -e "${RED}FAIL [$label]: empty response${NC}"
    exit 1
  fi
}

require_actor_namespace() {
  local label="$1"
  local response="$2"
  if ! RESPONSE_JSON="$response" EXPECTED_APP_ID="$APP_ID" python3 - <<'PY'
import json
import os
import sys

raw = os.environ["RESPONSE_JSON"]
expected = os.environ["EXPECTED_APP_ID"]
try:
    top = json.loads(raw)
except Exception:
    sys.exit(1)
actor_id = top.get("actor_id", "")
if not actor_id or f"::{expected}@" not in actor_id:
    sys.exit(1)
PY
  then
    echo -e "${RED}FAIL [$label]: response did not come from expected application namespace '$APP_ID'${NC}"
    echo "  Response: $response"
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
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
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
echo "Step 1: Undeploy from all nodes, then deploy to entry node"
"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2
_deployed=0
for _attempt in 1 2 3; do
    response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
      "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$TEMP_CONFIG" 2>&1)
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

# ─── Step 2: Test inference worker directly ───────────────────────────────────
echo "Step 2: Test inference worker directly"
INFER_RESPONSE=$(ask_actor "$WORKER_ACTOR_PATH" 30 '{"op":"infer","request_id":"test-direct-1","input":"hello-world"}')

if ! echo "$INFER_RESPONSE" | grep -q '"status"'; then
  echo -e "${RED}Direct inference failed: $INFER_RESPONSE${NC}"
  exit 1
fi
require_actor_namespace "direct_infer" "$INFER_RESPONSE"
check_response_field "direct_infer" "$INFER_RESPONSE" "status"
echo -e "  ${GREEN}Inference worker responded OK${NC}"

# ─── Step 3: Test get_metrics on inference worker ─────────────────────────────
echo "Step 3: Test get_metrics on inference worker"
METRICS_RESPONSE=$(ask_actor "$WORKER_ACTOR_PATH" 30 '{"op":"get_metrics"}')

require_nonempty "get_metrics" "$METRICS_RESPONSE"
require_actor_namespace "get_metrics" "$METRICS_RESPONSE"
check_response_field "get_metrics" "$METRICS_RESPONSE" "requests_processed"
echo -e "  ${GREEN}Metrics endpoint OK${NC}"

# ─── Step 4: Test shard benchmark (2 shards, 5 requests each) ─────────────────
echo "Step 4: Test shard benchmark (2 shards, 5 requests each)"
SHARD_BENCH_RESPONSE=$(ask_actor "$BENCHMARK_ACTOR_PATH" 120 '{"op":"run_shard_benchmark","shard_counts":[1,2],"requests_per_shard":5,"warmup_requests":1}')
require_nonempty "shard_benchmark" "$SHARD_BENCH_RESPONSE"
require_actor_namespace "shard_benchmark" "$SHARD_BENCH_RESPONSE"

if ! echo "$SHARD_BENCH_RESPONSE" | grep -q '"benchmark"'; then
  echo -e "${RED}Shard benchmark failed: $SHARD_BENCH_RESPONSE${NC}"
  exit 1
fi
check_response_field "shard_benchmark" "$SHARD_BENCH_RESPONSE" "results"
echo -e "  ${GREEN}Shard benchmark OK${NC}"

# ─── Step 5: Scaling benchmark ────────────────────────────────────────────────
echo "Step 5: Scaling benchmark"
SCALING_PAYLOAD=$(python3 - <<PY
import json
shards = [int(part.strip()) for part in "${SCALING_SHARDS}".split(",") if part.strip()]
print(json.dumps({
    "op": "run_scaling_benchmark",
    "shard_counts": shards,
    "requests_per_shard": int("${SCALING_REQUESTS_PER_SHARD}"),
    "warmup_requests": int("${SCALING_WARMUP_REQUESTS}"),
    "logical_actor_count": int("${SCALING_LOGICAL_ACTORS}"),
    "payload_size_bytes": int("${SCALING_DATA_SIZE_BYTES}"),
    "model_type": "${SCALING_MODEL_TYPE}",
    "work_multiplier": int("${SCALING_WORK_MULTIPLIER}")
}))
PY
)
SCALING_RESPONSE=$(ask_actor "$BENCHMARK_ACTOR_PATH" 300 "$SCALING_PAYLOAD")
require_nonempty "scaling_benchmark" "$SCALING_RESPONSE"
require_actor_namespace "scaling_benchmark" "$SCALING_RESPONSE"
check_response_field "scaling_benchmark" "$SCALING_RESPONSE" "results"
SCALING_CONFIG="actors=${SCALING_LOGICAL_ACTORS}  data=$(( SCALING_DATA_SIZE_BYTES / 1024 ))KB/req  model=${SCALING_MODEL_TYPE}  work_multiplier=${SCALING_WORK_MULTIPLIER}  rounds=${SCALING_REQUESTS_PER_SHARD}  nodes=${#NODE_LIST[@]}"
SCALING_RESPONSE_JSON="$SCALING_RESPONSE" SCALING_CONFIG="$SCALING_CONFIG" python3 - <<'PY'
import json, os
envelope = json.loads(os.environ["SCALING_RESPONSE_JSON"])
payload = envelope.get("payload", envelope)
if isinstance(payload, str):
    payload = json.loads(payload)
rows = payload.get("results", [])
if rows:
    print(f"  Config: {os.environ.get('SCALING_CONFIG', '')}")
    print()
    print(f"  {'Shards':>6}  {'Batch':>5}  {'TotalReq':>8}  {'KB/req':>6}  {'Req/s':>7}  {'Wall ms':>7}  {'p50':>5}  {'p95':>5}  {'p99':>5}  {'Compute ms':>10}  {'Coord ms':>8}  {'Comp%':>5}  {'Gran':>5}  {'Eff%':>5}  {'Nodes':>5}  {'Errs':>4}")
    print("  " + "-" * 120)
    for row in rows:
        kb = row.get('payload_size_bytes', 0) / 1024
        print(
            f"  {row.get('shards', 0):>6}  "
            f"{row.get('batch_size', 1):>5}  "
            f"{row.get('total_requests', 0):>8}  "
            f"{kb:>6.1f}  "
            f"{row.get('throughput_rps', 0):>7.1f}  "
            f"{row.get('wall_time_ms', 0):>7}  "
            f"{row.get('p50_latency_ms', 0):>5}  "
            f"{row.get('p95_latency_ms', 0):>5}  "
            f"{row.get('p99_latency_ms', 0):>5}  "
            f"{row.get('compute_time_ms', 0):>10}  "
            f"{row.get('coordination_time_ms', 0):>8}  "
            f"{row.get('compute_pct', 0):>5.1f}  "
            f"{row.get('granularity_ratio', 0):>5.2f}  "
            f"{row.get('parallel_efficiency_pct', 0):>5.1f}  "
            f"{row.get('worker_node_count', 0):>5}  "
            f"{row.get('error_count', 0):>4}"
        )
    print()
    print("  Legend:")
    print("    Batch    = logical actors processed per shard per scatter_gather round-trip")
    print("    TotalReq = Shards × Batch × Rounds (total logical inferences)")
    print("    KB/req   = payload size per request (data per inference)")
    print("    Comp%    = compute_time / (compute_time + coord_time) × 100  (higher = less coordination waste)")
    print("    Gran     = compute_time / coord_time  (granularity ratio; >1.0 means compute dominates)")
    print("    Eff%     = actual_throughput / ideal_linear_throughput × 100  (100% = perfect linear scaling)")
    print()
    print("  Scaling modes:")
    print("    Step 5  (strong scaling) — fixed total work, more shards → smaller batch per shard.")
    print("                              Throughput should rise then fall. Measures coordination overhead.")
    print("    Step 5b (weak scaling)   — fixed work per shard, more shards → larger total problem.")
    print("                              Throughput should stay flat (Eff%≈100) if the system scales well.")
PY

# ─── Step 5b: Weak scaling benchmark ─────────────────────────────────────────
echo "Step 5b: Weak scaling benchmark (fixed work per shard, problem grows with shards)"
WEAK_PAYLOAD=$(python3 - <<PY
import json
shards = [int(part.strip()) for part in "${SCALING_SHARDS}".split(",") if part.strip()]
print(json.dumps({
    "op": "run_weak_scaling_benchmark",
    "shard_counts": shards,
    "requests_per_shard": int("${SCALING_REQUESTS_PER_SHARD}"),
    "warmup_requests": int("${SCALING_WARMUP_REQUESTS}"),
    "logical_actor_count": int("${SCALING_LOGICAL_ACTORS}") // len(shards),
    "payload_size_bytes": int("${SCALING_DATA_SIZE_BYTES}"),
    "model_type": "${SCALING_MODEL_TYPE}",
    "work_multiplier": int("${SCALING_WORK_MULTIPLIER}")
}))
PY
)
WEAK_RESPONSE=$(ask_actor "$BENCHMARK_ACTOR_PATH" 300 "$WEAK_PAYLOAD")
require_nonempty "weak_scaling_benchmark" "$WEAK_RESPONSE"
require_actor_namespace "weak_scaling_benchmark" "$WEAK_RESPONSE"
check_response_field "weak_scaling_benchmark" "$WEAK_RESPONSE" "results"
WEAK_ACTORS_PER_SHARD=$(( SCALING_LOGICAL_ACTORS / $(echo "$SCALING_SHARDS" | tr ',' '\n' | wc -l | tr -d ' ') ))
WEAK_CONFIG="actors_per_shard=${WEAK_ACTORS_PER_SHARD}  data=$(( SCALING_DATA_SIZE_BYTES / 1024 ))KB/req  model=${SCALING_MODEL_TYPE}  work_multiplier=${SCALING_WORK_MULTIPLIER}  rounds=${SCALING_REQUESTS_PER_SHARD}  nodes=${#NODE_LIST[@]}"
SCALING_RESPONSE_JSON="$WEAK_RESPONSE" SCALING_CONFIG="$WEAK_CONFIG" python3 - <<'PY'
import json, os
envelope = json.loads(os.environ["SCALING_RESPONSE_JSON"])
payload = envelope.get("payload", envelope)
if isinstance(payload, str):
    payload = json.loads(payload)
rows = payload.get("results", [])
if rows:
    print(f"  Config: {os.environ.get('SCALING_CONFIG', '')}  (batch=constant → ideal Eff%=100 at all shard counts)")
    print()
    print(f"  {'Shards':>6}  {'Batch':>5}  {'TotalReq':>8}  {'KB/req':>6}  {'Req/s':>7}  {'Wall ms':>7}  {'p50':>5}  {'p95':>5}  {'p99':>5}  {'Compute ms':>10}  {'Coord ms':>8}  {'Comp%':>5}  {'Gran':>5}  {'Eff%':>5}  {'Nodes':>5}  {'Errs':>4}")
    print("  " + "-" * 120)
    for row in rows:
        kb = row.get('payload_size_bytes', 0) / 1024
        print(
            f"  {row.get('shards', 0):>6}  "
            f"{row.get('batch_size', 1):>5}  "
            f"{row.get('total_requests', 0):>8}  "
            f"{kb:>6.1f}  "
            f"{row.get('throughput_rps', 0):>7.1f}  "
            f"{row.get('wall_time_ms', 0):>7}  "
            f"{row.get('p50_latency_ms', 0):>5}  "
            f"{row.get('p95_latency_ms', 0):>5}  "
            f"{row.get('p99_latency_ms', 0):>5}  "
            f"{row.get('compute_time_ms', 0):>10}  "
            f"{row.get('coordination_time_ms', 0):>8}  "
            f"{row.get('compute_pct', 0):>5.1f}  "
            f"{row.get('granularity_ratio', 0):>5.2f}  "
            f"{row.get('parallel_efficiency_pct', 0):>5.1f}  "
            f"{row.get('worker_node_count', 0):>5}  "
            f"{row.get('error_count', 0):>4}"
        )
PY
echo -e "  ${GREEN}Weak scaling benchmark OK${NC}"

# ─── Step 6: Test collective benchmark ────────────────────────────────────────
echo "Step 6: Test collective benchmark"
COLLECTIVE_RESPONSE=$(ask_actor "$BENCHMARK_ACTOR_PATH" 120 '{"op":"run_collective_benchmark","num_shards":2}')
require_nonempty "collective_benchmark" "$COLLECTIVE_RESPONSE"
require_actor_namespace "collective_benchmark" "$COLLECTIVE_RESPONSE"

if ! echo "$COLLECTIVE_RESPONSE" | grep -q '"benchmark"'; then
  echo -e "${RED}Collective benchmark failed: $COLLECTIVE_RESPONSE${NC}"
  exit 1
fi
check_response_field "collective_benchmark" "$COLLECTIVE_RESPONSE" "timings"
echo -e "  ${GREEN}Collective benchmark OK${NC}"

# ─── Step 7: Test orchestrator workflow — shard mode ──────────────────────────
echo "Step 7: Test orchestrator workflow (shard mode)"
ORCH_SHARD_RESPONSE=$(ask_actor "$ORCHESTRATOR_ACTOR_PATH" 120 '{"op":"workflow_run","mode":"shard","num_shards":2,"num_requests":4}')
require_nonempty "orchestrator_shard" "$ORCH_SHARD_RESPONSE"
require_actor_namespace "orchestrator_shard" "$ORCH_SHARD_RESPONSE"

if ! echo "$ORCH_SHARD_RESPONSE" | grep -q '"status"'; then
  echo -e "${RED}Orchestrator shard mode failed: $ORCH_SHARD_RESPONSE${NC}"
  exit 1
fi
check_response_field "orchestrator_shard" "$ORCH_SHARD_RESPONSE" "total_processed"
echo -e "  ${GREEN}Orchestrator shard mode OK${NC}"

# ─── Step 8: Test orchestrator workflow — collective mode ─────────────────────
echo "Step 8: Test orchestrator workflow (collective mode)"
ORCH_COLLECTIVE_RESPONSE=$(ask_actor "$ORCHESTRATOR_ACTOR_PATH" 120 '{"op":"workflow_run","mode":"collective","num_shards":2,"num_requests":4}')
require_nonempty "orchestrator_collective" "$ORCH_COLLECTIVE_RESPONSE"
require_actor_namespace "orchestrator_collective" "$ORCH_COLLECTIVE_RESPONSE"

if ! echo "$ORCH_COLLECTIVE_RESPONSE" | grep -q '"status"'; then
  echo -e "${RED}Orchestrator collective mode failed: $ORCH_COLLECTIVE_RESPONSE${NC}"
  exit 1
fi
check_response_field "orchestrator_collective" "$ORCH_COLLECTIVE_RESPONSE" "timings"
echo -e "  ${GREEN}Orchestrator collective mode OK${NC}"

# ─── Step 9: Query orchestrator status ────────────────────────────────────────
echo "Step 9: Query orchestrator status"
STATUS_RESPONSE=$(ask_actor "$ORCHESTRATOR_ACTOR_PATH" 30 '{"op":"workflow_query:status"}')
require_nonempty "orchestrator_query" "$STATUS_RESPONSE"
require_actor_namespace "orchestrator_query" "$STATUS_RESPONSE"

check_response_field "orchestrator_query" "$STATUS_RESPONSE" "mode"
check_response_field "orchestrator_query" "$STATUS_RESPONSE" "total_processed"
echo -e "  ${GREEN}Orchestrator status query OK${NC}"

# ─── Step 10: Check application metrics ───────────────────────────────────────
echo "Step 10: Check application metrics"
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
print(f"  application_id={app.get('application_id', 'n/a')}  status={app.get('status', 'n/a')}")
print(f"  message_count={int(metrics.get('message_count', 0))}  error_count={int(metrics.get('error_count', 0))}")
counters = metrics.get("counter_metrics", {})
if counters:
    print("  Counters:")
    for k in sorted(counters):
        print(f"    {k}={counters[k]}")
totals = metrics.get("latency_totals_ms", {})
maxes  = metrics.get("latency_max_ms", {})
samples = metrics.get("latency_samples", {})
if totals:
    print("  Latency (ms):")
    for k in sorted(totals):
        n = int(samples.get(k, 0))
        avg = totals[k] / n if n > 0 else 0
        print(f"    {k}: total={totals[k]}  max={maxes.get(k, 0)}  samples={n}  avg={avg:.1f}")
PY
echo -e "  ${GREEN}Metrics check OK${NC}"

# ─── Step 11: Get all benchmark results ───────────────────────────────────────
echo "Step 11: Retrieve all benchmark results"
ALL_RESULTS=$(ask_actor "$BENCHMARK_ACTOR_PATH" 30 '{"op":"get_results"}')
require_nonempty "get_results" "$ALL_RESULTS"
require_actor_namespace "get_results" "$ALL_RESULTS"

check_response_field "get_results" "$ALL_RESULTS" "total_benchmarks"
echo -e "  ${GREEN}All benchmark results retrieved OK${NC}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Python parallel AI inference example passed.${NC}"
echo "  Mechanisms tested:"
echo "    1. ShardGroup scatter-gather (shard benchmark, orchestrator shard mode)"
echo "    2. Elastic pool checkout/checkin (run_pool_benchmark)"
echo "    3. MPI collectives (broadcast, barrier, reduce, allreduce)"
echo "    4. Process groups (orchestrator workflow coordination)"
