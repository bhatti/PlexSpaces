#!/bin/bash
# Test Payment Workflow - Go WASM (Cadence-style)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/payment_workflow_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="cadence-payment-workflow-go"
ACTOR_TYPE="payment-workflow"
NUM_PAYMENTS=80

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

cleanup() {
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "================================================================"
echo "  Payment Workflow (Go WASM - Cadence-style)"
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
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=cadence-payment-workflow-go" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "${RED}Deploy failed (HTTP $HTTP_CODE): $RESPONSE${NC}"
    exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
    local id="$1" payload="$2" timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$id/ask?timeout=$timeout" \
        -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Single payment (workflow_run)"
RUN=$(send_op "pay-1" '{"op":"workflow_run","payment_id":"pay-1","idempotency_key":"key-1","amount_cents":9999}' 15)
if echo "$RUN" | grep -q '"status":"settled"'; then
    echo -e "  ${GREEN}Payment pay-1 settled${NC}"
else
    echo "  Response: $RUN"
fi

echo "Step 3: Idempotency (same idempotency_key returns same result)"
RUN2=$(send_op "pay-1" '{"op":"workflow_run","payment_id":"pay-1","idempotency_key":"key-1","amount_cents":9999}' 15)
if echo "$RUN2" | grep -q '"message":"already_completed"'; then
    echo -e "  ${GREEN}Idempotency: duplicate request ignored${NC}"
fi

echo "Step 4: Query status"
QUERY=$(send_op "pay-1" '{"op":"workflow_query:status"}' 5)
echo "  Query: $(echo "$QUERY" | head -c 140)..."

echo "Step 5: Refund signal (pay-2)"
send_op "pay-2" '{"op":"workflow_run","payment_id":"pay-2","idempotency_key":"key-2","amount_cents":5000}' 15 >/dev/null
send_op "pay-2" '{"op":"workflow_signal:refund"}' 5 >/dev/null
Q2=$(send_op "pay-2" '{"op":"workflow_query:status"}' 5)
if echo "$Q2" | grep -q '"refund_requested":true'; then
    echo -e "  ${GREEN}Refund signal received${NC}"
fi

echo "Step 6: Batch $NUM_PAYMENTS payments (capture last Run response for metrics)"
BATCH_START=$(date +%s%N)
LAST_RUN_RESP=""
for i in $(seq 1 $NUM_PAYMENTS); do
    RESP=$(send_op "pay-batch-$i" "{\"op\":\"workflow_run\",\"payment_id\":\"pay-batch-$i\",\"idempotency_key\":\"key-batch-$i\",\"amount_cents\":10000}" 15)
    if [ "$i" -eq "$NUM_PAYMENTS" ]; then
        LAST_RUN_RESP="$RESP"
    fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Payments/sec: $(echo "scale=1; $NUM_PAYMENTS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 7: Metrics from last payment (from workflow_run response)"
echo "================================================================"
STATS_RESP="$LAST_RUN_RESP"
if echo "$STATS_RESP" | grep -q '"payment_id"'; then
    export WALL_MS NUM_PAYMENTS
    echo "$STATS_RESP" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    try: p = json.loads(p)
    except: p = {}
payment_id = p.get('payment_id', 'N/A')
status = p.get('status', 'N/A')
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
steps = p.get('steps_count', 0) or p.get('steps_completed', 0)
retry_count = p.get('retry_count', 0) or 0
total_ms = compute_ms + coord_ms
if total_ms > 0:
    compute_pct = 100 * compute_ms / total_ms
    coord_pct = 100 * coord_ms / total_ms
    gran = compute_ms / coord_ms if coord_ms > 0 else 0
else:
    compute_pct = coord_pct = gran = 0
wall_ms = int(os.environ.get('WALL_MS', 0))
num_payments = int(os.environ.get('NUM_PAYMENTS', 1))
payments_per_sec = num_payments / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print('  Payment (sample)')
print('  ────────────────────────────────────────────')
print(f'  Payment ID:         {payment_id}')
print(f'  Status:             {status}')
print(f'  Steps completed:    {steps}')
print(f'  Retry count:        {retry_count}')
print()
print('  Benchmarks (coord vs compute)')
print('  ────────────────────────────────────────────')
print(f'  Compute time:       {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination time:  {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:       {gran:.1f}x (compute/coordinate)')
print()
print(f'  Batch total wall:   {wall_ms}ms')
print(f'  Payments:          {num_payments}')
print(f'  Payments/sec:      {payments_per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $STATS_RESP)"
else
    echo "  Response: $STATS_RESP"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Payment Workflow (Go - Cadence) Test Complete${NC}"
echo "================================================================"
