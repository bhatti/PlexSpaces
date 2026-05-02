#!/bin/bash
# Test V8 Isolates log processor (TypeScript WASM): batch processing, throughput, metrics
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/log_processor_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-v8-isolates-log-processor-ts"
ACTOR_TYPE="log-processor"
INSTANCE_ID="default"
LINES_PER_BATCH=200
NUM_BATCHES=80
BATCH_RUNS=50

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  V8 Isolates → PlexSpaces: High-throughput log processor"
echo "  Batch processing, parse level, throughput metrics"
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
  -F "name=migrating-v8-isolates-log-processor-ts" \
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
  local payload="$1" timeout="${2:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${INSTANCE_ID}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Build sample log lines (mixed levels)"
LINES_JSON="["
for i in $(seq 0 $((LINES_PER_BATCH - 1))); do
  [ "$i" -gt 0 ] && LINES_JSON+=","
  case $((i % 4)) in
    0) LINES_JSON+="\"INFO request completed in ${i}ms\"" ;;
    1) LINES_JSON+="\"WARN high latency ${i}\"" ;;
    2) LINES_JSON+="\"ERROR connection refused ${i}\"" ;;
    *) LINES_JSON+="\"DEBUG trace id=${i}\"" ;;
  esac
done
LINES_JSON+="]"

echo "Step 3: Send batches ($NUM_BATCHES batches × $LINES_PER_BATCH lines)"
BATCH_START=$(date +%s%N)
LAST_RESP=""
for i in $(seq 1 $NUM_BATCHES); do
  RESP=$(send_op "{\"op\":\"process_batch\",\"lines\":$LINES_JSON}" 30)
  if [ "$i" -eq "$NUM_BATCHES" ]; then LAST_RESP="$RESP"; fi
  if [ $((i % 20)) -eq 0 ]; then
    echo "  Sent $i/$NUM_BATCHES batches..."
  fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batches sent. Wall: ${WALL_MS}ms${NC}"

echo "Step 4: Extra batch runs (2+ s total)"
for i in $(seq 1 $BATCH_RUNS); do
  send_op "{\"op\":\"process_batch\",\"lines\":$LINES_JSON}" 30 >/dev/null
done
EXTRA_END=$(date +%s%N)
TOTAL_WALL_MS=$(( (EXTRA_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Total wall: ${TOTAL_WALL_MS}ms${NC}"

echo "Step 5: Status and metrics"
echo "================================================================"
STATUS=$(send_op '{"op":"status"}' 10)
if echo "$STATUS" | grep -q '"processed_count"'; then
  echo "$STATUS" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
proc = p.get('processed_count', 0)
batches = p.get('batches_received', 0)
total_bytes = p.get('total_bytes', 0)
by_level = p.get('by_level', {})
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
elapsed_ms = p.get('elapsed_ms', 0) or 1
events_per_sec = p.get('events_per_sec', 0) or (proc / (elapsed_ms / 1000) if elapsed_ms else 0)
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
print()
print('  Log processor (V8 Isolates style)')
print('  High-throughput batch processing')
print('  ────────────────────────────────────────────')
print(f'  Processed:       {proc} lines')
print(f'  Batches:         {batches}')
print(f'  Total bytes:     {total_bytes}')
print(f'  By level:        {by_level}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Events/sec:      {events_per_sec:.1f}')
print(f'  Compute ms:     {compute_ms:.1f} ({compute_pct:.1f}%)')
print(f'  Coord ms:       {coord_ms:.1f} ({coord_pct:.1f}%)')
print(f'  Elapsed ms:     {elapsed_ms}')
" 2>/dev/null || echo "  (Parse: $STATUS)"
else
  echo "  Response: $STATUS"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}V8 Isolates log processor test complete${NC}"
echo "================================================================"
