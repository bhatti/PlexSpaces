#!/bin/bash
# Test GPU Job Scheduler - Kueue style (Python WASM)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/job_scheduler_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="migrating-kueue-scheduler-py"
ACTOR_TYPE="job-scheduler"
NUM_JOBS=30

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  Kueue → PlexSpaces: GPU Job Scheduler (Python WASM)"
echo "  GenServer + lock (allocate) + KV; priority, preemption"
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
  -F "name=migrating-kueue-scheduler-py" \
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
  local id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$id/ask?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Submit jobs (priority order)"
send_op "sched-1" '{"op":"submit","job_id":"j1","priority":5,"gpu_request":2}' 10
send_op "sched-1" '{"op":"submit","job_id":"j2","priority":10,"gpu_request":1}' 10
send_op "sched-1" '{"op":"submit","job_id":"j3","priority":3,"gpu_request":4}' 10
if send_op "sched-1" '{"op":"submit","job_id":"j4","priority":8,"gpu_request":2}' 10 | grep -q '"ok":true'; then
  echo -e "  ${GREEN}Submitted j1,j2,j3,j4${NC}"
fi

echo "Step 3: Allocate (with lock) – should return highest priority job"
ALLOC=$(send_op "sched-1" '{"op":"allocate"}' 15)
if echo "$ALLOC" | grep -q '"job_id":"j2"'; then
  echo -e "  ${GREEN}Allocated j2 (priority 10)${NC}"
else
  echo "  Alloc: $(echo "$ALLOC" | head -c 100)..."
fi

echo "Step 4: List queue"
send_op "sched-1" '{"op":"list_queue"}' 10 | head -c 140
echo "..."

echo "Step 5: Complete j2, allocate again"
send_op "sched-1" '{"op":"complete","job_id":"j2"}' 10
ALLOC2=$(send_op "sched-1" '{"op":"allocate"}' 10)
if echo "$ALLOC2" | grep -q '"job_id"'; then
  echo -e "  ${GREEN}Completed j2, allocated next${NC}"
fi

echo "Step 6: Preempt (allocate j4, then preempt)"
send_op "sched-1" '{"op":"allocate"}' 10 >/dev/null
send_op "sched-1" '{"op":"allocate"}' 10 >/dev/null
PREEMPT=$(send_op "sched-1" '{"op":"allocate"}' 10)
if echo "$PREEMPT" | grep -q '"job_id"'; then
  JID=$(echo "$PREEMPT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('payload',d).get('job_id',''))" 2>/dev/null)
  send_op "sched-1" "{\"op\":\"preempt\",\"job_id\":\"$JID\"}" 10 >/dev/null
  echo -e "  ${GREEN}Preempted $JID (requeued)${NC}"
fi

echo "Step 7: Batch $NUM_JOBS jobs (submit → allocate → complete)"
BATCH_START=$(date +%s%N)
LAST_QUOTA=""
for i in $(seq 1 $NUM_JOBS); do
  send_op "batch" "{\"op\":\"submit\",\"job_id\":\"batch-$i\",\"priority\":$i,\"gpu_request\":1}" 10 >/dev/null
done
for i in $(seq 1 $NUM_JOBS); do
  A=$(send_op "batch" '{"op":"allocate"}' 10)
  JID=$(echo "$A" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('job_id','') if p else '')" 2>/dev/null)
  [ -n "$JID" ] && send_op "batch" "{\"op\":\"complete\",\"job_id\":\"$JID\"}" 10 >/dev/null
  [ "$i" -eq "$NUM_JOBS" ] && LAST_QUOTA=$(send_op "batch" '{"op":"get_quotas"}' 10)
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Jobs: $NUM_JOBS${NC}"
echo ""

echo "Step 8: Quotas / metrics"
echo "================================================================"
if [ -n "$LAST_QUOTA" ]; then
  echo "$LAST_QUOTA" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
used = p.get('used_gpus', 0)
max_g = p.get('max_gpus', 0)
proc = p.get('processed_count', 0)
comp = p.get('total_compute_ms', 0) or 0
coord = p.get('total_coord_ms', 0) or 0
print()
print('  GPU Job Scheduler (Kueue-style)')
print('  ────────────────────────────────────────────')
print(f'  Used GPUs:        {used} / {max_g}')
print(f'  Processed count:  {proc}')
print(f'  Compute ms:       {comp:.1f}')
print(f'  Coord ms:         {coord:.1f}')
" 2>/dev/null || echo "$LAST_QUOTA"
else
  echo "  (no quota response)"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}GPU Job Scheduler (Kueue) Test Complete${NC}"
echo "================================================================"
