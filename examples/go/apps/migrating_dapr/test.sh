#!/bin/bash
# Test Job Processor - Go WASM (Dapr-style)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/job_processor_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-dapr-job-processor-go"
ACTOR_TYPE="job-processor"
NUM_JOBS=50

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'


echo "================================================================"
echo "  Job Processor (Go WASM - Dapr-style)"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node. Start: ./scripts/server.sh${NC}"
    exit 1
fi

echo "Step 1: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
      -F "application_id=$APP_ID" \
      -F "name=migrating-dapr-job-processor-go" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$CONFIG_FILE" 2>&1)
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
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
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
    local id="$1" payload="$2" timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$id/ask?timeout=$timeout" \
        -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Enqueue + process (workflow_run)"
ENQ=$(send_op "default" '{"op":"workflow_run","action":"enqueue","job_id":"j1","payload":{"task":"notify"}}' 10)
if echo "$ENQ" | grep -q '"status":"enqueued"'; then
    echo -e "  ${GREEN}Job j1 enqueued${NC}"
fi
PROC=$(send_op "default" '{"op":"workflow_run","action":"process"}' 15)
echo "  Process: $(echo "$PROC" | head -c 120)..."

echo "Step 3: Query status"
QUERY=$(send_op "default" '{"op":"workflow_query:status"}' 5)
echo "  Query: $(echo "$QUERY" | head -c 140)..."

echo "Step 4: Cancel signal"
send_op "q2" '{"op":"workflow_run","queue_id":"q2","action":"enqueue","job_id":"j-cancel"}' 10 >/dev/null
send_op "q2" '{"op":"workflow_signal:cancel"}' 5 >/dev/null
Q2=$(send_op "q2" '{"op":"workflow_query:status"}' 5)
if echo "$Q2" | grep -q '"cancel_requested":true'; then
    echo -e "  ${GREEN}Cancel received${NC}"
fi

echo "Step 5: Batch enqueue + process $NUM_JOBS jobs"
BATCH_START=$(date +%s%N)
LAST_RESP=""
for i in $(seq 1 $NUM_JOBS); do
    send_op "batch" "{\"op\":\"workflow_run\",\"queue_id\":\"batch\",\"action\":\"enqueue\",\"job_id\":\"batch-$i\"}" 10 >/dev/null
    RESP=$(send_op "batch" '{"op":"workflow_run","action":"process"}' 20)
    if [ "$i" -eq "$NUM_JOBS" ]; then
        LAST_RESP="$RESP"
    fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Jobs/sec: $(echo "scale=1; $NUM_JOBS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 6: Metrics from last run"
echo "================================================================"
STATS_RESP="$LAST_RESP"
if echo "$STATS_RESP" | grep -q '"queue_id"'; then
    export WALL_MS NUM_JOBS
    echo "$STATS_RESP" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    try: p = json.loads(p)
    except: p = {}
queue_id = p.get('queue_id', 'N/A')
status = p.get('status', 'N/A')
queue_depth = p.get('queue_depth', 0) or 0
dlq_size = p.get('dlq_size', 0) or 0
processed = p.get('processed_count', 0) or 0
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
if total_ms > 0:
    compute_pct = 100 * compute_ms / total_ms
    coord_pct = 100 * coord_ms / total_ms
    gran = compute_ms / coord_ms if coord_ms > 0 else 0
else:
    compute_pct = coord_pct = gran = 0
wall_ms = int(os.environ.get('WALL_MS', 0))
num_jobs = int(os.environ.get('NUM_JOBS', 1))
jobs_per_sec = num_jobs / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print('  Job processor (sample)')
print('  ────────────────────────────────────────────')
print(f'  Queue ID:          {queue_id}')
print(f'  Status:            {status}')
print(f'  Queue depth:       {queue_depth}')
print(f'  DLQ size:          {dlq_size}')
print(f'  Processed count:   {processed}')
print()
print('  Benchmarks (coord vs compute)')
print('  ────────────────────────────────────────────')
print(f'  Compute time:      {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:      {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:       {gran:.1f}x (compute/coordinate)')
print()
print(f'  Batch wall:        {wall_ms}ms')
print(f'  Jobs:              {num_jobs}')
print(f'  Jobs/sec:          {jobs_per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $STATS_RESP)"
else
    echo "  Response: $STATS_RESP"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Job Processor (Go - Dapr) Test Complete${NC}"
echo "================================================================"
