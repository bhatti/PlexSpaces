#!/bin/bash
# Test parameter sweep - Merlin style (Python WASM), worker pool with elastic size
# Uses elastic pool API (pool_checkout/send/pool_checkin) when pool is configured;
# falls back to process group broadcast. sweep:coord-1 = coordinator; sweep:worker-0,1,... = workers.
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/sweep_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-merlin-sweep-py"
ACTOR_TYPE="sweep"
NUM_PARAMS=80
BATCH_SWEEPS=10
BATCH_PARAMS_PER_SWEEP=24
POOL_SIZE_FIRST=2
POOL_SIZE_ELASTIC=5

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  Merlin → PlexSpaces: Parameter sweep (worker pool, elastic)"
echo "  Pool API (checkout/checkin) + host.ts (work queue); fallback: process group"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
  echo ""
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 1: Deploy"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_ID" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"; exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
  local instance_id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${instance_id}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Wake worker pool (size $POOL_SIZE_FIRST) – workers join merlin-workers"
for i in $(seq 0 $((POOL_SIZE_FIRST - 1))); do
  send_op "worker-$i" '{"op":"work_available","sweep_id":"","num_params":0}' 10 >/dev/null
done
echo -e "  ${GREEN}Pool size: $POOL_SIZE_FIRST workers${NC}"

echo "Step 3: Run parameter sweep (coord-1 scatters $NUM_PARAMS, broadcasts; workers take/run/write; coord gathers)"
RUN1=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"sweep_id\":\"sweep-1\",\"num_params\":$NUM_PARAMS}" 45)
if echo "$RUN1" | grep -q '"status":"completed"'; then
  echo -e "  ${GREEN}Sweep sweep-1 completed ($NUM_PARAMS params, pool=$POOL_SIZE_FIRST)${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 200)..."
fi

echo "Step 4: Query coordinator status"
send_op "coord-1" '{"op":"workflow_query:status"}' 5 | head -c 140
echo "..."

echo "Step 5: Elastic scale – wake 3 more workers (pool size $POOL_SIZE_ELASTIC)"
for i in $(seq 2 $((POOL_SIZE_ELASTIC - 1))); do
  send_op "worker-$i" '{"op":"work_available","sweep_id":"","num_params":0}' 10 >/dev/null
done
echo -e "  ${GREEN}Pool size: $POOL_SIZE_ELASTIC workers (elastic)${NC}"

echo "Step 6: Second sweep with larger pool"
RUN2=$(send_op "coord-1" '{"op":"workflow_run","sweep_id":"sweep-2","num_params":12}' 30)
if echo "$RUN2" | grep -q '"num_completed"'; then
  echo -e "  ${GREEN}Sweep sweep-2 done (pool=$POOL_SIZE_ELASTIC)${NC}"
fi

echo "Step 7: Batch $BATCH_SWEEPS sweeps ($BATCH_PARAMS_PER_SWEEP params/sweep)"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $BATCH_SWEEPS); do
  RESP=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"sweep_id\":\"batch-$i\",\"num_params\":$BATCH_PARAMS_PER_SWEEP}" 60)
  if [ "$i" -eq "$BATCH_SWEEPS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
TOTAL_PARAMS=$(( BATCH_SWEEPS * BATCH_PARAMS_PER_SWEEP ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Sweeps: $BATCH_SWEEPS, Params: $TOTAL_PARAMS, Pool: $POOL_SIZE_ELASTIC${NC}"
echo ""

echo "Step 8: Metrics from last run"
echo "================================================================"
if [ -n "$LAST_RUN" ] && echo "$LAST_RUN" | grep -q '"success":true'; then
  export WALL_MS
  export POOL_SIZE_ELASTIC
  export BATCH_SWEEPS
  export BATCH_PARAMS_PER_SWEEP
  export TOTAL_PARAMS
  echo "$LAST_RUN" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
if not isinstance(p, dict):
  p = {}
if p.get('error'):
  print('  Last run error:', p.get('error'))
  sys.exit(0)
if not p.get('sweep_id'):
  print('  (No sweep_id in last response)')
  sys.exit(0)
sid = p.get('sweep_id', 'N/A')
status = p.get('status', 'N/A')
num_params = p.get('num_params', 0)
num_done = p.get('num_completed', 0)
data_bytes = p.get('data_size_bytes', 0) or (num_params * 100 + num_done * 80)
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
wall_ms = int(os.environ.get('WALL_MS', 0))
pool_size = int(os.environ.get('POOL_SIZE_ELASTIC', 0))
batch_sweeps = int(os.environ.get('BATCH_SWEEPS', 10))
batch_params = int(os.environ.get('BATCH_PARAMS_PER_SWEEP', 24))
total_params = int(os.environ.get('TOTAL_PARAMS', batch_sweeps * batch_params))
wall_sec = wall_ms / 1000.0 if wall_ms else 0.001
req_per_sec = batch_sweeps / wall_sec if wall_sec else 0
params_per_sec = total_params / wall_sec if wall_sec else 0
print()
print('  Parameter sweep (Merlin-style)')
print('  Worker pool (elastic size)')
print('  ────────────────────────────────────────────')
print(f'  Sweep ID:         {sid}')
print(f'  Status:           {status}')
print(f'  Params:           {num_done} / {num_params}')
print(f'  Worker pool size: {pool_size} workers')
print(f'  Data size:        {num_params} params, ~{data_bytes:,} bytes')
print()
print('  Throughput')
print('  ────────────────────────────────────────────')
print(f'  Req/sec:          {req_per_sec:.2f} (sweeps/sec)')
print(f'  Params/sec:       {params_per_sec:.1f}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Compute ms:       {compute_ms:.1f} ({compute_pct:.1f}%)')
print(f'  Coord ms:         {coord_ms:.1f} ({coord_pct:.1f}%)')
print(f'  Batch wall:       {wall_ms}ms')
" 2>/dev/null || echo "  (Parse: $LAST_RUN)"
else
  echo "  Response: $LAST_RUN"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Parameter sweep (Merlin) Test Complete${NC}"
echo "================================================================"
