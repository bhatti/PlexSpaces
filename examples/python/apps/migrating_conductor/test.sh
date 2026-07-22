#!/bin/bash
# Test Order Saga Workflow - Python WASM (Netflix Conductor/Orkes style)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/order_saga_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-conductor-order-saga-py"
ACTOR_TYPE="order-saga"
NUM_ORDERS=50

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'



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

echo "================================================================"
echo "  Order Saga Workflow (Python WASM - Conductor/Orkes style)"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
fi

HTTP_CHECK="000"
for _i in 1 2 3; do
  trap 'rm -f "${APP_ZIP:-}"' EXIT
  APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
  HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
  [ "$HTTP_CHECK" != "000" ] && break
  sleep 2
done
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
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=migrating-conductor-order-saga-py" \
      -F "version=1.0.0" \
      -F "app_file=@$APP_ZIP" 2>&1) || true
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
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Single order (workflow_run)"
RUN=$(send_op "order-1" '{"op":"workflow_run","order_id":"order-1"}' 25)
if echo "$RUN" | grep -q '"status":"completed"'; then
    echo -e "  ${GREEN}Order order-1 completed${NC}"
else
    echo "  Response: $(echo "$RUN" | head -c 160)..."
fi

echo "Step 3: Query status"
QUERY=$(send_op "order-1" '{"op":"workflow_query:status"}' 5)
echo "  Query: $(echo "$QUERY" | head -c 140)..."

echo "Step 4: Simulate failure + compensation (fail at inventory)"
RUN_FAIL=$(send_op "order-fail-1" '{"op":"workflow_run","order_id":"order-fail-1","simulate_fail_at":"update_inventory"}' 25)
if echo "$RUN_FAIL" | grep -q '"compensation_done":true'; then
    echo -e "  ${GREEN}Compensation ran after failure at update_inventory${NC}"
else
    echo "  Response: $(echo "$RUN_FAIL" | head -c 160)..."
fi

echo "Step 5: Cancel signal (order-2)"
send_op "order-2" '{"op":"workflow_run","order_id":"order-2"}' 25 >/dev/null
send_op "order-2" '{"op":"workflow_signal:cancel"}' 5 >/dev/null
Q2=$(send_op "order-2" '{"op":"workflow_query:status"}' 5)
if echo "$Q2" | grep -q '"cancel_requested":true'; then
    echo -e "  ${GREEN}Cancel received${NC}"
fi

echo "Step 6: Batch $NUM_ORDERS orders (capture last Run for metrics)"
BATCH_START=$(date +%s%N)
LAST_RUN_RESP=""
for i in $(seq 1 $NUM_ORDERS); do
    RESP=$(send_op "order-batch-$i" "{\"op\":\"workflow_run\",\"order_id\":\"order-batch-$i\"}" 30)
    if [ "$i" -eq "$NUM_ORDERS" ]; then
        LAST_RUN_RESP="$RESP"
    fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Orders/sec: $(echo "scale=1; $NUM_ORDERS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 7: Metrics from last order (workflow_run response)"
echo "================================================================"
STATS_RESP="$LAST_RUN_RESP"
if echo "$STATS_RESP" | grep -q '"order_id"'; then
    export WALL_MS NUM_ORDERS
    echo "$STATS_RESP" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    try: p = json.loads(p)
    except: p = {}
order_id = p.get('order_id', 'N/A')
status = p.get('status', 'N/A')
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
steps = p.get('steps_completed', 0)
total_ms = compute_ms + coord_ms
if total_ms > 0:
    compute_pct = 100 * compute_ms / total_ms
    coord_pct = 100 * coord_ms / total_ms
    gran = compute_ms / coord_ms if coord_ms > 0 else 0
else:
    compute_pct = coord_pct = gran = 0
wall_ms = int(os.environ.get('WALL_MS', 0))
num_orders = int(os.environ.get('NUM_ORDERS', 1))
orders_per_sec = num_orders / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print('  Order saga (sample)')
print('  ────────────────────────────────────────────')
print(f'  Order ID:          {order_id}')
print(f'  Status:           {status}')
print(f'  Steps completed:  {steps}')
print()
print('  Benchmarks (coord vs compute)')
print('  ────────────────────────────────────────────')
print(f'  Compute time:     {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:    {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:      {gran:.1f}x (compute/coordinate)')
print()
print(f'  Batch wall:       {wall_ms}ms')
print(f'  Orders:           {num_orders}')
print(f'  Orders/sec:       {orders_per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $STATS_RESP)"
else
    echo "  Response: $STATS_RESP"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Order Saga Workflow (Python - Conductor) Test Complete${NC}"
echo "================================================================"
