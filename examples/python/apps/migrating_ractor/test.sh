#!/bin/bash
# Test Ractor Calculator - Type-Safe Actor Message Passing (Python WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8091)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
# (HTTP gateway will be on 8091 + 1 = 8091)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/ractor_calculator.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="ractor-calc"
BATCH_COUNT=10000



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
echo "  Ractor Calculator - Type-Safe Actor Message Passing"
echo "  PlexSpaces Python WASM App"
echo "================================================================"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node
echo "Step 1: Check node status"
echo "----------------------------------------------------------------"
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "Start node first: PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:$((HTTP_PORT - 1))"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""


# Deploy
echo "Step 2: Deploy calculator actor"
echo "----------------------------------------------------------------"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
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

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}Deployed $APP_ID${NC}"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper
send_op() {
    local actor="$1"
    local payload="$2"
    local timeout="${3:-10}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Test arithmetic operations (Ractor-style: actor.call(Add { a: 10, b: 5 }))
echo "Step 3: Test arithmetic operations (Ractor-style message passing)"
echo "----------------------------------------------------------------"

PASS=0
FAIL=0

test_op() {
    local op="$1"
    local a="$2"
    local b="$3"
    local expected="$4"

    RESP=$(send_op "calculator" "{\"op\":\"$op\",\"a\":$a,\"b\":$b}")
    RESULT=$(echo "$RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
print(p.get('result', 'ERROR'))
" 2>/dev/null || echo "ERROR")

    if [ "$RESULT" = "$expected" ]; then
        echo -e "  ${GREEN}OK${NC} $op($a, $b) = $RESULT"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}FAIL${NC} $op($a, $b) = $RESULT (expected $expected)"
        FAIL=$((FAIL + 1))
    fi
}

test_op "add" 10 5 "15.0"
test_op "subtract" 10 5 "5.0"
test_op "multiply" 10 5 "50.0"
test_op "divide" 10 5 "2.0"
test_op "add" 100 200 "300.0"
test_op "multiply" 3 7 "21.0"
test_op "divide" 100 4 "25.0"

# Division by zero
RESP=$(send_op "calculator" '{"op":"divide","a":10,"b":0}')
if echo "$RESP" | grep -q "Division by zero"; then
    echo -e "  ${GREEN}OK${NC} divide(10, 0) = Division by zero (caught)"
    PASS=$((PASS + 1))
else
    echo -e "  ${RED}FAIL${NC} divide(10, 0) should return error"
    FAIL=$((FAIL + 1))
fi

echo ""
echo "  Results: $PASS passed, $FAIL failed"
echo ""

# Check history
echo "Step 4: Verify operation history"
echo "----------------------------------------------------------------"
HIST_RESP=$(send_op "calculator" '{"op":"get_history"}')
OP_COUNT=$(echo "$HIST_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
print(p.get('operation_count', 0))
" 2>/dev/null || echo "?")
echo -e "  Operations recorded: $OP_COUNT"
echo ""

# Batch benchmark
echo "Step 5: Batch benchmark ($BATCH_COUNT operations)"
echo "----------------------------------------------------------------"

BATCH_START=$(date +%s%N)
BATCH_RESP=$(send_op "calculator" "{\"op\":\"batch\",\"count\":$BATCH_COUNT}" 60)
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))

echo "$BATCH_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
total = p.get('total_operations', 0)
dur = p.get('duration_ms', 0)
ops = p.get('ops_per_sec', 0)
if isinstance(dur, str): dur = float(dur)
if isinstance(ops, str): ops = float(ops)
print(f'  Operations:    {total:,}')
print(f'  WASM time:     {dur:,.0f} ms')
print(f'  Throughput:    {ops:,.0f} ops/sec')
" 2>/dev/null || echo "  (could not parse batch results)"
echo "  Wall clock:    ${WALL_MS} ms"
echo ""

# Final stats
echo "Step 6: Final statistics"
echo "----------------------------------------------------------------"
STATS_RESP=$(send_op "calculator" '{"op":"stats"}')
echo "$STATS_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
counters = p.get('counters', {})
bench = p.get('benchmarks', {})
print(f'  Actor ID:         {p.get(\"actor_id\", \"?\")}')
print(f'  Operations:       {counters.get(\"operation_count\", 0)}')
print(f'  Batch ops:        {counters.get(\"batch_ops\", 0):,}')
print(f'  Total compute:    {bench.get(\"total_compute_ms\", 0):,.0f} ms')
print(f'  Ops/sec:          {bench.get(\"ops_per_sec\", 0):,.0f}')
" 2>/dev/null || echo "  (could not parse stats)"

echo ""
echo "================================================================"
echo ""
echo "  This example demonstrated:"
echo "    - Ractor-style typed message passing (@handler per operation)"
echo "    - Actor state isolation (operation count, history)"
echo "    - Batch benchmarking for throughput measurement"
echo ""
echo "  Ractor equivalent:"
echo "    actor.call(CalculatorMessage::Add { a: 10.0, b: 5.0, reply: port })"
echo "  PlexSpaces:"
echo "    send_op(\"calculator\", '{\"op\":\"add\",\"a\":10,\"b\":5}')"
echo ""
echo "================================================================"
