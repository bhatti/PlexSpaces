#!/bin/bash
# Test LuaTS-style event pipeline (Python WASM): TupleSpace + CDC events
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/pipeline_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="migrating-luats-pipeline-py"
ACTOR_TYPE="pipeline"
NUM_EVENTS=250
BATCH_RUNS=5
EVENTS_PER_RUN=120

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  LuaTS → PlexSpaces: Event-driven data pipeline (CDC)"
echo "  TupleSpace event buffer + Linda-style take/read by pattern"
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
  -F "name=migrating-luats-pipeline-py" \
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

echo "Step 2: Run pipeline (write $NUM_EVENTS events to TupleSpace, take by pattern, aggregate)"
RUN1=$(send_op "run-1" "{\"op\":\"workflow_run\",\"pipeline_id\":\"run-1\",\"num_events\":$NUM_EVENTS}" 45)
if echo "$RUN1" | grep -q '"status":"completed"'; then
  echo -e "  ${GREEN}Pipeline run-1 completed ($NUM_EVENTS events)${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 200)..."
fi

echo "Step 3: Query status"
send_op "run-1" '{"op":"workflow_query:status"}' 5 | head -c 160
echo "..."

echo "Step 4: Publish single events (Linda-style write), then window_flush"
for i in 0 1 2; do
  send_op "run-1" "{\"op\":\"publish\",\"source\":\"test\",\"seq\":$i,\"payload\":{\"x\":$i}}" 5 >/dev/null
done
send_op "run-1" '{"op":"window_flush","pipeline_id":"run-1"}' 10 >/dev/null
echo -e "  ${GREEN}Published 3 events + window_flush${NC}"

echo "Step 5: Batch runs (2+ seconds)"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $BATCH_RUNS); do
  RESP=$(send_op "run-1" "{\"op\":\"workflow_run\",\"pipeline_id\":\"batch-$i\",\"num_events\":$EVENTS_PER_RUN}" 60)
  if [ "$i" -eq "$BATCH_RUNS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Runs: $BATCH_RUNS, Events/run: $EVENTS_PER_RUN${NC}"
echo ""

echo "Step 6: Metrics from last run"
echo "================================================================"
if [ -n "$LAST_RUN" ] && echo "$LAST_RUN" | grep -q '"success":true'; then
  export WALL_MS
  export BATCH_RUNS
  export EVENTS_PER_RUN
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
if not p.get('pipeline_id'):
  print('  (No pipeline_id in last response; raw payload may be incomplete)')
  sys.exit(0)
pid = p.get('pipeline_id', 'N/A')
status = p.get('status', 'N/A')
written = p.get('events_written', 0)
processed = p.get('events_processed', 0)
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
wall_ms = int(os.environ.get('WALL_MS', 0))
batch_runs = int(os.environ.get('BATCH_RUNS', 5))
events_per_run = int(os.environ.get('EVENTS_PER_RUN', 120))
wall_sec = wall_ms / 1000.0 if wall_ms else 0.001
events_per_sec = (batch_runs * events_per_run) / wall_sec if wall_sec else 0
print()
print('  Event pipeline (LuaTS-style CDC)')
print('  TupleSpace + Linda take/read by pattern')
print('  ────────────────────────────────────────────')
print(f'  Pipeline ID:      {pid}')
print(f'  Status:           {status}')
print(f'  Events written:   {written}')
print(f'  Events processed: {processed}')
print()
print('  Throughput')
print('  ────────────────────────────────────────────')
print(f'  Events/sec:       {events_per_sec:.1f}')
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
echo -e "  ${GREEN}LuaTS event pipeline test complete${NC}"
echo "================================================================"
