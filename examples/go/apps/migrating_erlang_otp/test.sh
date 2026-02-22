#!/bin/bash
# Test Rate Limiter - Sliding Window Rate Limiting Service (Go WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 7993)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992
# (HTTP gateway will be on 7992 + 1 = 7993)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/rate_limiter.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-7993}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="erlang-otp-rl"
MAX_REQUESTS=100
WINDOW_MS=60000
BATCH_SIZE=5000

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "================================================================"
echo "  Erlang/OTP Rate Limiter - Sliding Window"
echo "  PlexSpaces Go WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:   localhost:$HTTP_PORT"
echo "  Window:         ${WINDOW_MS}ms ($(( WINDOW_MS / 1000 ))s)"
echo "  Max requests:   $MAX_REQUESTS per window"
echo "  Batch test:     $BATCH_SIZE requests"
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
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "Start node first: PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:$((HTTP_PORT - 1))"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""

trap cleanup EXIT

# Deploy
echo "Step 2: Deploy rate limiter actor"
echo "----------------------------------------------------------------"

curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=erlang-otp-rate-limiter" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}Deployed $APP_ID${NC}"
    echo "  - Rate limiter: rate-limiter"
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
    local timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor?timeout=$timeout" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Verify actor
echo "Step 3: Verify rate limiter actor"
echo "----------------------------------------------------------------"

RL_RESP=$(send_op "rate-limiter" '{"op":"stats"}')
if echo "$RL_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}Rate limiter actor OK${NC}"
else
    echo -e "  ${RED}Rate limiter failed: $RL_RESP${NC}"
    exit 1
fi
echo ""

# Test single client rate limiting
echo "Step 4: Single Client Rate Limiting Test"
echo "----------------------------------------------------------------"
echo ""

echo "  Sending $MAX_REQUESTS requests (should all be allowed)..."
ALLOWED=0
DENIED=0
for i in $(seq 1 $MAX_REQUESTS); do
    RESP=$(send_op "rate-limiter" "{\"op\":\"check_rate\",\"client_id\":\"test-client-1\"}" 5)
    if echo "$RESP" | grep -q '"allowed":true'; then
        ALLOWED=$((ALLOWED + 1))
    else
        DENIED=$((DENIED + 1))
    fi
done

echo -e "  ${GREEN}Allowed: $ALLOWED${NC}  ${RED}Denied: $DENIED${NC}"

if [ "$ALLOWED" -eq "$MAX_REQUESTS" ] && [ "$DENIED" -eq 0 ]; then
    echo -e "  ${GREEN}PASS: All $MAX_REQUESTS requests allowed${NC}"
else
    echo -e "  ${YELLOW}WARN: Expected $MAX_REQUESTS allowed, got $ALLOWED${NC}"
fi
echo ""

# Test rate limit enforcement
echo "  Sending 10 more requests (should be denied)..."
DENIED_COUNT=0
for i in $(seq 1 10); do
    RESP=$(send_op "rate-limiter" "{\"op\":\"check_rate\",\"client_id\":\"test-client-1\"}" 5)
    if echo "$RESP" | grep -q '"allowed":false'; then
        DENIED_COUNT=$((DENIED_COUNT + 1))
    fi
done

echo -e "  ${RED}Denied: $DENIED_COUNT/10${NC}"
if [ "$DENIED_COUNT" -eq 10 ]; then
    echo -e "  ${GREEN}PASS: Rate limit enforced${NC}"
else
    echo -e "  ${YELLOW}WARN: Expected 10 denied, got $DENIED_COUNT${NC}"
fi
echo ""

# Test client isolation
echo "Step 5: Client Isolation Test"
echo "----------------------------------------------------------------"

RESP=$(send_op "rate-limiter" '{"op":"check_rate","client_id":"test-client-2"}' 5)
if echo "$RESP" | grep -q '"allowed":true'; then
    echo -e "  ${GREEN}PASS: Different client has independent limit${NC}"
else
    echo -e "  ${RED}FAIL: Client isolation broken${NC}"
fi
echo ""

# Batch benchmark
echo "Step 6: Batch Benchmark ($BATCH_SIZE requests across 50 clients)"
echo "----------------------------------------------------------------"
echo ""

BENCH_START=$(date +%s%N)

BATCH_RESP=$(send_op "rate-limiter" "{\"op\":\"check_rate_batch\",\"client_id\":\"bench\",\"count\":$BATCH_SIZE}" 120)

BENCH_END=$(date +%s%N)
WALL_MS=$(( (BENCH_END - BENCH_START) / 1000000 ))

if echo "$BATCH_RESP" | grep -q '"status":"ok"'; then
    echo "$BATCH_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
total = p.get('total_requests', 0)
allowed = p.get('allowed', 0)
denied = p.get('denied', 0)
dur = p.get('duration_ms', 0)
ops = p.get('ops_per_sec', 0)
clients = p.get('unique_clients', 0)
if isinstance(dur, str): dur = float(dur)
if isinstance(ops, str): ops = float(ops)
print(f'  Total requests:  {total}')
print(f'  Allowed:         {allowed}')
print(f'  Denied:          {denied}')
print(f'  Unique clients:  {clients}')
print(f'  Duration:        {dur:.1f}ms')
print(f'  Throughput:      {ops:,.0f} ops/sec')
" 2>/dev/null || echo "  (Could not parse batch results)"
else
    echo -e "  ${RED}Batch benchmark failed: $BATCH_RESP${NC}"
fi
echo ""

# Get final stats
echo "Step 7: Final Statistics & Benchmarks"
echo "================================================================"
echo ""

STATS_RESP=$(send_op "rate-limiter" '{"op":"stats"}')

if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo "$STATS_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
config = p.get('config', {})
counters = p.get('counters', {})
bench = p.get('benchmarks', {})

print('  Rate Limit Configuration')
print('  ────────────────────────────────────────────')
print(f'  Window:              {config.get(\"window_ms\", 0)}ms')
print(f'  Max requests:        {config.get(\"max_requests\", 0)} per window')
print()
print('  Counters')
print('  ────────────────────────────────────────────')
print(f'  Total checks:        {counters.get(\"total_checks\", 0):,}')
print(f'  Total allowed:       {counters.get(\"total_allowed\", 0):,}')
print(f'  Total denied:        {counters.get(\"total_denied\", 0):,}')
deny_rate = counters.get('deny_rate_pct', 0)
if isinstance(deny_rate, str): deny_rate = float(deny_rate)
print(f'  Deny rate:           {deny_rate:.1f}%')
print(f'  Active clients:      {counters.get(\"active_clients\", 0)}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
total_ms = bench.get('total_ms', 0)
compute_ms = bench.get('compute_ms', 0)
coord_ms = bench.get('coord_ms', 0)
ops = bench.get('ops_per_sec', 0)
gran = bench.get('granularity', 0)
mem = bench.get('memory_kb', 0)
entries = bench.get('entries_total', 0)
if isinstance(total_ms, str): total_ms = float(total_ms)
if isinstance(compute_ms, str): compute_ms = float(compute_ms)
if isinstance(coord_ms, str): coord_ms = float(coord_ms)
if isinstance(ops, str): ops = float(ops)
if isinstance(gran, str): gran = float(gran)
if isinstance(mem, str): mem = float(mem)
compute_pct = bench.get('compute_pct', 0)
coord_pct = bench.get('coord_pct', 0)
if isinstance(compute_pct, str): compute_pct = float(compute_pct)
if isinstance(coord_pct, str): coord_pct = float(coord_pct)
print(f'  Total time:          {total_ms:.1f}ms')
print(f'  Compute time:        {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination time:   {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:         {gran:.1f}x (compute/coordinate)')
print(f'  Throughput:          {ops:,.0f} ops/sec')
print(f'  Memory:              {mem:.1f} KB ({entries} window entries)')
" 2>/dev/null || echo "  (Could not parse stats)"
else
    echo -e "  ${RED}Stats failed: $STATS_RESP${NC}"
fi

echo ""
echo -e "${BOLD}Wall clock time: ${WALL_MS}ms${NC}"
echo ""
echo "================================================================"
echo -e "  ${GREEN}Erlang/OTP Rate Limiter Test Complete${NC}"
echo "================================================================"
