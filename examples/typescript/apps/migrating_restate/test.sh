#!/bin/bash
# Test Exactly-Once Payment - Restate style (TypeScript WASM)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/payment_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-restate-payment-ts"
ACTOR_TYPE="payment"
NUM_PAYMENTS=50

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
echo "  Restate → PlexSpaces: Exactly-Once Payment (TypeScript WASM)"
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
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=migrating-restate-payment-ts" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
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

echo "Step 2: Single payment (workflow_run)"
RUN=$(send_op "svc-1" '{"op":"workflow_run","idempotency_key":"key-1","amount_cents":9999,"from_account":"acc-a","to_account":"acc-b"}' 15)
if echo "$RUN" | grep -q '"status":"confirmed"'; then
  echo -e "  ${GREEN}Payment key-1 confirmed${NC}"
else
  echo "  Response: $(echo "$RUN" | head -c 120)..."
fi

echo "Step 3: Idempotency (duplicate key-1 returns cached)"
RUN2=$(send_op "svc-1" '{"op":"workflow_run","idempotency_key":"key-1","amount_cents":9999}' 15)
if echo "$RUN2" | grep -q '"message":"already_completed"'; then
  echo -e "  ${GREEN}Exactly-once: duplicate key-1 returned cached result${NC}"
fi

echo "Step 4: Query status"
send_op "svc-1" '{"op":"workflow_query:status"}' 5 | head -c 120
echo "..."

echo "Step 5: Cancel signal (svc-2)"
send_op "svc-2" '{"op":"workflow_run","idempotency_key":"key-cancel"}' 15 >/dev/null
send_op "svc-2" '{"op":"workflow_signal:cancel"}' 5 >/dev/null
Q2=$(send_op "svc-2" '{"op":"workflow_query:status"}' 5)
if echo "$Q2" | grep -q '"cancel_requested":true'; then
  echo -e "  ${GREEN}Cancel received${NC}"
fi

echo "Step 6: Batch $NUM_PAYMENTS payments (capture last Run for metrics)"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $NUM_PAYMENTS); do
  RESP=$(send_op "batch" "{\"op\":\"workflow_run\",\"idempotency_key\":\"batch-$i\",\"amount_cents\":10000}" 15)
  if [ "$i" -eq "$NUM_PAYMENTS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Payments/sec: $(echo "scale=1; $NUM_PAYMENTS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 7: Metrics from last run"
echo "================================================================"
STATS_RESP="$LAST_RUN"
if echo "$STATS_RESP" | grep -q '"idempotency_key"'; then
  export WALL_MS NUM_PAYMENTS
  echo "$STATS_RESP" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
key = p.get('idempotency_key', 'N/A')
status = p.get('status', 'N/A')
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
steps = p.get('steps_completed', 0) or 0
total_ms = compute_ms + coord_ms
if total_ms > 0:
  compute_pct = 100 * compute_ms / total_ms
  coord_pct = 100 * coord_ms / total_ms
  gran = compute_ms / coord_ms if coord_ms > 0 else 0
else:
  compute_pct = coord_pct = gran = 0
wall_ms = int(os.environ.get('WALL_MS', 0))
n = int(os.environ.get('NUM_PAYMENTS', 1))
per_sec = n / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print('  Payment (sample)')
print('  ────────────────────────────────────────────')
print(f'  Idempotency key:   {key}')
print(f'  Status:           {status}')
print(f'  Steps completed:  {steps}')
print()
print('  Benchmarks (coord vs compute)')
print('  ────────────────────────────────────────────')
print(f'  Compute time:     {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:     {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:      {gran:.1f}x')
print()
print(f'  Batch wall:       {wall_ms}ms')
print(f'  Payments:         {n}')
print(f'  Payments/sec:     {per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $STATS_RESP)"
else
  echo "  Response: $STATS_RESP"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Exactly-Once Payment (Restate) Test Complete${NC}"
echo "================================================================"
