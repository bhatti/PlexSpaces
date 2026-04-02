#!/bin/bash
# Test Dirigo windowed stream (TypeScript WASM): run batch, ingest, window_flush, metrics
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/stream_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="migrating-dirigo-stream-ts"
ACTOR_TYPE="stream"
NUM_EVENTS=80
BATCH_RUNS=5
EVENTS_PER_RUN=40

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  Dirigo → PlexSpaces: Windowed stream (TypeScript WASM)"
echo "  Real-time analytics, Kafka-style window aggregation"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  chmod +x "$SCRIPT_DIR/build.sh"
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
  -F "name=migrating-dirigo-stream-ts" \
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

echo "Step 2: Run workflow with event batch (window_size=10)"
EVENTS_JSON="["
for i in $(seq 0 $((NUM_EVENTS - 1))); do
  [ "$i" -gt 0 ] && EVENTS_JSON+=","
  EVENTS_JSON+="{\"event_id\":\"e-$i\",\"value\":$((20 + i % 60)),\"ts\":$((1000 + i))}"
done
EVENTS_JSON+="]"
RUN1=$(send_op "run-1" "{\"op\":\"workflow_run\",\"stream_id\":\"run-1\",\"window_size\":10,\"events\":$EVENTS_JSON}" 30)
if echo "$RUN1" | grep -q '"processed_count"'; then
  echo -e "  ${GREEN}Stream run-1 processed events (windowed aggregation)${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 200)..."
fi

echo "Step 3: Query status"
send_op "run-1" '{"op":"workflow_query:status"}' 5 | head -c 180
echo "..."

echo "Step 4: Ingest single events + window_flush"
for i in 1 2 3 4 5; do
  send_op "run-1" "{\"op\":\"ingest\",\"event_id\":\"s-$i\",\"value\":$((10 * i))}" 5 >/dev/null
done
send_op "run-1" '{"op":"window_flush"}' 5 >/dev/null
echo -e "  ${GREEN}Ingested 5 events + window_flush${NC}"

echo "Step 5: Batch runs (2+ seconds)"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $BATCH_RUNS); do
  EV="["
  for j in $(seq 0 $((EVENTS_PER_RUN - 1))); do
    [ "$j" -gt 0 ] && EV+=","
    EV+="{\"event_id\":\"b${i}-$j\",\"value\":$((30 + j))}"
  done
  EV+="]"
  RESP=$(send_op "run-1" "{\"op\":\"workflow_run\",\"stream_id\":\"batch-$i\",\"events\":$EV}" 30)
  if [ "$i" -eq "$BATCH_RUNS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Runs: $BATCH_RUNS, Events/run: $EVENTS_PER_RUN${NC}"
echo ""

echo "Step 6: Metrics from last run"
echo "================================================================"
if [ -n "$LAST_RUN" ] && echo "$LAST_RUN" | grep -q '"stream_id"'; then
  echo "$LAST_RUN" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
sid = p.get('stream_id', 'N/A')
status = p.get('status', 'N/A')
processed = p.get('processed_count', 0)
windows = p.get('windows_emitted', 0)
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
print()
print('  Windowed stream (Dirigo-style)')
print('  Kafka-style windowed aggregation')
print('  ────────────────────────────────────────────')
print(f'  Stream ID:        {sid}')
print(f'  Status:           {status}')
print(f'  Processed:       {processed}')
print(f'  Windows emitted: {windows}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Compute ms:      {compute_ms:.1f} ({compute_pct:.1f}%)')
print(f'  Coord ms:        {coord_ms:.1f} ({coord_pct:.1f}%)')
" 2>/dev/null || echo "  (Parse: $LAST_RUN)"
else
  echo "  Response: $LAST_RUN"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Dirigo windowed stream test complete${NC}"
echo "================================================================"
