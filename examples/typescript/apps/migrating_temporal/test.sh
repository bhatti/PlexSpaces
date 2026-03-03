#!/bin/bash
# Test Order Fulfillment Workflow - E-commerce saga (TypeScript WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8092)
#
# Prerequisites: Start PlexSpaces node first:
#   ./scripts/server.sh (from repo root)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/order_fulfillment_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="temporal-order-fulfillment-ts"
ACTOR_TYPE="order-fulfillment"
NUM_ORDERS=50

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "================================================================"
echo "  Temporal → PlexSpaces: Order Fulfillment Workflow"
echo "  TypeScript WASM (run / signal / query)"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:   localhost:$HTTP_PORT"
echo "  Orders:         $NUM_ORDERS"
echo ""

if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
    echo ""
fi

echo "Step 1: Check node status"
echo "----------------------------------------------------------------"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "Start node first: ./scripts/server.sh (from repo root)"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""

trap cleanup EXIT

echo "Step 2: Deploy order fulfillment workflow"
echo "----------------------------------------------------------------"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=temporal-order-fulfillment-ts" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')

if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "${GREEN}Deployed $APP_ID${NC}"
    echo "  - OrderFulfillment: order-fulfillment (Workflow + virtual_actor + durability)"
else
    echo -e "${RED}Deploy failed (HTTP $HTTP_CODE): $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

send_op() {
    local instance_id="$1"
    local payload="$2"
    local timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$instance_id?timeout=$timeout" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 3: Single order workflow (run → query)"
echo "----------------------------------------------------------------"
RUN_RESP=$(send_op "order-1" '{"op":"workflow_run","order_id":"order-1","customer_id":"cust-1"}' 15)
if echo "$RUN_RESP" | grep -q '"status":"shipped"'; then
    echo -e "  ${GREEN}Order order-1 completed (shipped)${NC}"
elif echo "$RUN_RESP" | grep -q '"status":"cancelled"'; then
    echo -e "  ${YELLOW}Order order-1 cancelled${NC}"
else
    echo -e "  ${RED}Run failed: $RUN_RESP${NC}"
fi

# Query status (workflow_query:status - client sends op for routing)
QUERY_RESP=$(send_op "order-1" '{"op":"workflow_query:status"}' 5)
if echo "$QUERY_RESP" | grep -q '"status":"shipped"'; then
    echo -e "  ${GREEN}Query status OK${NC}"
else
    echo -e "  Response: $QUERY_RESP"
fi
echo ""

echo "Step 4: Cancel signal (run partial → signal cancel → compensate)"
echo "----------------------------------------------------------------"
# Start order-2 (will run steps)
send_op "order-2" '{"op":"workflow_run","order_id":"order-2","customer_id":"cust-2"}' 15 >/dev/null
# Send cancel signal (workflow_signal:cancel)
SIGNAL_RESP=$(send_op "order-2" '{"op":"workflow_signal:cancel"}' 5)
# Query to see cancelled
QUERY2=$(send_op "order-2" '{"op":"workflow_query:status"}' 5)
if echo "$QUERY2" | grep -q '"cancel_requested":true'; then
    echo -e "  ${GREEN}Cancel signal received; state updated${NC}"
else
    echo -e "  ${YELLOW}Query: $QUERY2${NC}"
fi
echo ""

echo "Step 5: Batch orders ($NUM_ORDERS orders, 2+ s target)"
echo "----------------------------------------------------------------"
BATCH_START=$(date +%s%N)
for i in $(seq 1 $NUM_ORDERS); do
    inst="order-batch-$i"
    send_op "$inst" "{\"op\":\"workflow_run\",\"order_id\":\"$inst\",\"customer_id\":\"cust-$i\"}" 15 >/dev/null 2>&1 || true
    if [ $((i % 10)) -eq 0 ]; then
        echo "  Processed $i/$NUM_ORDERS orders..."
    fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch complete${NC}"
echo "  Wall clock: ${WALL_MS}ms"
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
echo -e "  ${GREEN}Order Fulfillment Workflow (TypeScript) Test Complete${NC}"
echo "================================================================"
