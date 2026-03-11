#!/usr/bin/env bash
# Test SkyPilot multi-cloud scheduler (Rust WASM). GenServer: submit_task, get_best_resources, get_status.
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/scheduler_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

echo "Step 0: Build WASM (ensure latest with metrics)"
"$SCRIPT_DIR/build.sh" || { echo -e "\033[0;31mBuild failed\033[0m"; exit 1; }
echo ""

APP_ID="migrating-skypilot-scheduler-rust"
ACTOR_TYPE="scheduler"
INSTANCE_ID="default"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  SkyPilot → PlexSpaces: Multi-cloud ML scheduler (Rust WASM)"
echo "================================================================"

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
  -F "name=migrating-skypilot-scheduler-rust" \
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
  local payload="$1" timeout="${2:-15}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${INSTANCE_ID}?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Submit ML training task (GPU required, cost-optimized)"
RUN1=$(send_op '{"op":"submit_task","task":{"task_id":"training-1","task_type":"training","gpu_required":true,"gpu_memory_gb":16,"cpu_cores":4,"memory_gb":16,"cloud_preference":null}}' 15)
if echo "$RUN1" | grep -q '"allocation"'; then
  echo -e "  ${GREEN}Training task scheduled${NC}"
else
  echo "  Response: $RUN1"
fi

echo "Step 3: Submit inference task (CPU-only)"
RUN2=$(send_op '{"op":"submit_task","task":{"task_id":"inference-1","task_type":"inference","gpu_required":false,"gpu_memory_gb":0,"cpu_cores":4,"memory_gb":15,"cloud_preference":null}}' 15)
if echo "$RUN2" | grep -q '"allocation"'; then
  echo -e "  ${GREEN}Inference task scheduled${NC}"
else
  echo "  Response: $RUN2"
fi

echo "Step 4: Get best resources (pre-flight)"
RUN3=$(send_op '{"op":"get_best_resources","task":{"task_id":"preflight","task_type":"training","gpu_required":true,"gpu_memory_gb":16,"cpu_cores":4,"memory_gb":16,"cloud_preference":null}}' 15)
if echo "$RUN3" | grep -q '"allocation"'; then
  echo -e "  ${GREEN}Resource recommendation OK${NC}"
fi

echo "Step 5: Get status"
STATUS=$(send_op '{"op":"get_status"}' 10)
echo "  Status: $STATUS"

echo "Step 6: Batch submit (20 tasks, ~2+ s)"
BATCH_START=$(date +%s%N)
for i in $(seq 1 20); do
  send_op "{\"op\":\"submit_task\",\"task\":{\"task_id\":\"batch-$i\",\"task_type\":\"training\",\"gpu_required\":true,\"gpu_memory_gb\":16,\"cpu_cores\":4,\"memory_gb\":16,\"cloud_preference\":null}}" 10 >/dev/null
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms${NC}"

echo "Step 7: Metrics (coord vs compute)"
METRICS=$(send_op '{"op":"get_status"}' 10)
echo "$METRICS" | python3 -c "
import sys, json
raw = sys.stdin.read()
try:
    d = json.loads(raw)
except json.JSONDecodeError:
    print('  (Could not parse response)')
    sys.exit(0)
# HTTP response: { \"success\", \"payload\", \"actor_id\", ... }; payload is parsed JSON from actor
p = d.get('payload')
if p is None:
    p = d
if isinstance(p, str):
    try:
        p = json.loads(p)
    except json.JSONDecodeError:
        p = {}
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
if compute_ms == 0 and coord_ms == 0 and ('total_compute_ms' not in p and 'total_coord_ms' not in p):
    print('  Metrics not in response (payload has only queue_size/running).')
    print('  Rebuild and redeploy: ./build.sh then run this test again.')
else:
    total_ms = compute_ms + coord_ms
    compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
    coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
    gran = p.get('granularity_ratio') if 'granularity_ratio' in p else ((compute_ms / coord_ms) if coord_ms > 0 else 0)
    scheduled = p.get('tasks_scheduled', 0)
    running = p.get('running', 0)
    queue = p.get('queue_size', 0)
    print()
    print('  SkyPilot scheduler – coord / compute')
    print('  ────────────────────────────────────────────')
    print(f'  Tasks scheduled:  {scheduled}')
    print(f'  Running:          {running}  Queue: {queue}')
    print()
    print('  Benchmarks')
    print('  ────────────────────────────────────────────')
    print(f'  Compute ms:       {compute_ms:.1f} ({compute_pct:.1f}%)')
    print(f'  Coord ms:         {coord_ms:.1f} ({coord_pct:.1f}%)')
    print(f'  Granularity:      {gran:.1f}x (compute/coordinate)')
" 2>/dev/null || echo "  Response: $METRICS"

echo ""
echo "================================================================"
echo -e "  ${GREEN}SkyPilot scheduler (Rust) test complete${NC}"
echo "================================================================"
