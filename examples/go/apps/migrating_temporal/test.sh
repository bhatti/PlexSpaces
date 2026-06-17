#!/bin/bash
# Test Order Fulfillment Workflow - Go WASM
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/order_fulfillment_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="temporal-order-fulfillment-go"
ACTOR_TYPE="order-fulfillment"
NUM_ORDERS=50

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
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
echo "  Order Fulfillment Workflow (Go WASM)"
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
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=temporal-order-fulfillment-go" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$CONFIG_FILE" 2>&1) || true
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
RUN=$(send_op "order-1" '{"op":"workflow_run","order_id":"order-1","customer_id":"cust-1"}' 15)
if echo "$RUN" | grep -q '"status":"shipped"'; then
    echo -e "  ${GREEN}Order order-1 shipped${NC}"
else
    echo "  Response: $RUN"
fi

echo "Step 3: Query status"
QUERY=$(send_op "order-1" '{"op":"workflow_query:status"}' 5)
echo "  Query: $(echo "$QUERY" | head -c 120)..."

echo "Step 4: Cancel signal (order-2)"
send_op "order-2" '{"op":"workflow_run","order_id":"order-2","customer_id":"cust-2"}' 15 >/dev/null
send_op "order-2" '{"op":"workflow_signal:cancel"}' 5 >/dev/null
Q2=$(send_op "order-2" '{"op":"workflow_query:status"}' 5)
if echo "$Q2" | grep -q '"cancel_requested":true'; then
    echo -e "  ${GREEN}Cancel received${NC}"
fi

echo "Step 5: Batch $NUM_ORDERS orders"
BATCH_START=$(date +%s%N)
for i in $(seq 1 $NUM_ORDERS); do
    send_op "order-batch-$i" "{\"op\":\"workflow_run\",\"order_id\":\"order-batch-$i\",\"customer_id\":\"cust-$i\"}" 15 >/dev/null 2>&1 || true
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Orders/sec: $(echo "scale=1; $NUM_ORDERS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 6: Metrics from last order"
echo "================================================================"
STATS_RESP=$(send_op "order-batch-$NUM_ORDERS" '{"op":"workflow_query:status"}' 5)
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
compute_ms = p.get('total_compute_ms', 0)
coord_ms = p.get('total_coord_ms', 0)
steps = p.get('steps_count', 0)
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
print('  Order (sample)')
print('  ────────────────────────────────────────────')
print(f'  Order ID:           {order_id}')
print(f'  Status:             {status}')
print(f'  Steps completed:    {steps}')
print()
print('  Benchmarks (coord vs compute)')
print('  ────────────────────────────────────────────')
print(f'  Compute time:       {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination time:  {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:        {gran:.1f}x (compute/coordinate)')
print()
print(f'  Batch total wall:   {wall_ms}ms')
print(f'  Orders:             {num_orders}')
print(f'  Orders/sec:         {orders_per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $STATS_RESP)"
else
    echo "  Response: $STATS_RESP"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Order Fulfillment Workflow (Go) Test Complete${NC}"
echo "================================================================"
