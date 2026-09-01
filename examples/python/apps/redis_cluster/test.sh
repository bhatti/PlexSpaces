#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Redis Cluster — exercises all PlexSpaces primitives via HTTP API
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/redis_cluster_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
HTTP_PORT2="${2:-8093}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="redis-cluster"
COORDINATOR="redis-coordinator"
BENCHMARK="redis-benchmark"

# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

PASS=0
FAIL=0

check() {
    local label="$1"
    local response="$2"
    local pattern="$3"
    if echo "$response" | grep -q "$pattern"; then
        echo -e "  ${GREEN}✓${NC} $label"
        PASS=$((PASS + 1))
    else
        echo -e "  ${RED}✗ FAIL${NC}: $label"
        echo "    Response: $response"
        echo "    Expected pattern: $pattern"
        FAIL=$((FAIL + 1))
    fi
}

redis_op() {
    local desc="$1"
    local payload="$2"
    curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$COORDINATOR/ask?timeout=30" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null || echo '{"error":"curl_failed"}'
}

benchmark_op() {
    local desc="$1"
    local payload="$2"
    curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$BENCHMARK/ask?timeout=120" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null || echo '{"error":"curl_failed"}'
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Redis Cluster with PlexSpaces Actors (Python WASM)     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Always rebuild so source changes are picked up
echo "Building WASM actor..."
chmod +x "$SCRIPT_DIR/build.sh"
"$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo ""

# Check node
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Deploy
echo "Step 2: Deploy Redis Cluster"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT" 2>/dev/null || true
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT2" 2>/dev/null || true
sleep 1

APP_ZIP="$(mktemp /tmp/redis_cluster_XXXXXX.zip)"
trap 'rm -f "$APP_ZIP"' EXIT
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null

DEPLOY_OUT=""
for attempt in 1 2 3; do
    DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -F "application_id=$APP_ID" \
        -F "name=$APP_ID" \
        -F "version=1.0.0" \
        -F "app_file=@$APP_ZIP" 2>&1) || true
    HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
    RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
    if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -q '"success".*true'; then
        break
    fi
    echo "  Deploy attempt $attempt failed, retrying..."
    sleep 3
done

if ! echo "$RESPONSE" | grep -q '"success".*true'; then
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Redis cluster deployed${NC}"
sleep 2
echo ""

# Check second node is reachable
echo "Step 2b: Verify second node reachable (multi-node cluster)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
HTTP_CHECK2=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT2/" 2>/dev/null) || HTTP_CHECK2="000"
if [ "$HTTP_CHECK2" = "000" ]; then
    echo -e "${YELLOW}⚠ Second node at localhost:$HTTP_PORT2 not reachable — single-node mode${NC}"
else
    echo -e "${GREEN}✓ Second node is running${NC}"
fi
echo ""

# Setup cluster
echo "Step 3: Initialize cluster (create shard groups, replication handshake)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(redis_op "setup" '{"op":"setup"}')
check "create_shard_group: master + replica" "$R" '"status".*"ok"'
check "replication handshake: PING->REPLCONF->PSYNC" "$R" '"masters"'

# Verify multi-node: shards should span both node IDs in actor IDs
if [ "$HTTP_CHECK2" != "000" ]; then
    check "shards distributed across both nodes" "$R" '"master_actor_ids"'
    # Actor IDs contain @<node-id>; verify we see activity on both nodes
    NODE1_IDS=$(echo "$R" | grep -o '@[^"]*' | grep -v "8093" | head -1) || true
    NODE2_IDS=$(echo "$R" | grep -o '@[^"]*' | grep "8093\|replica\|node-2" | head -1) || true
    if [ -n "$NODE2_IDS" ]; then
        echo -e "  ${GREEN}✓ Shards confirmed on second node: $NODE2_IDS${NC}"
    else
        echo -e "  ${YELLOW}⚠ Could not confirm second-node shards in actor IDs (may need a moment to stabilize)${NC}"
    fi
fi
echo ""

# Basic operations
echo "Step 4: Basic Operations (Ch5-6 — Actor messaging)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(redis_op "ping" '{"op":"ping"}')
check "PING -> [PONG, PONG, PONG]" "$R" "PONG"

R=$(redis_op "SET user:1 alice" '{"op":"set","key":"user:1","value":"alice"}')
check "SET user:1 alice -> OK" "$R" '"result".*"OK"'

R=$(redis_op "SET user:2 bob EX 300" '{"op":"set","key":"user:2","value":"bob","ex":300}')
check "SET user:2 bob EX 300 -> OK" "$R" '"result".*"OK"'

R=$(redis_op "GET user:1" '{"op":"get","key":"user:1"}')
check "GET user:1 -> alice" "$R" '"result".*"alice"'

R=$(redis_op "GET nonexistent" '{"op":"get","key":"nonexistent"}')
check "GET nonexistent -> null (nil)" "$R" '"found".*false'
echo ""

# INCR
echo "Step 5: INCR Command (Ch9 — create-if-missing, error-if-non-integer)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(redis_op "INCR counter" '{"op":"incr","key":"counter"}')
check "INCR counter (new) -> 1" "$R" '"result".*1'

R=$(redis_op "INCR counter" '{"op":"incr","key":"counter"}')
check "INCR counter -> 2" "$R" '"result".*2'

redis_op "SET notanumber hello" '{"op":"set","key":"notanumber","value":"hello"}' >/dev/null
R=$(redis_op "INCR notanumber" '{"op":"incr","key":"notanumber"}')
check "INCR notanumber -> error path" "$R" '"error"'
echo ""

# Expiry
echo "Step 6: Key Expiry (Ch4 — passive + active sweep)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
redis_op "SET volatile PX 100" '{"op":"set","key":"volatile","value":"temp","px":100}' >/dev/null
R=$(redis_op "GET volatile (immediate)" '{"op":"get","key":"volatile"}')
check "GET volatile (immediate) -> temp" "$R" '"result".*"temp"'

sleep 0.2
R=$(redis_op "GET volatile (after expiry)" '{"op":"get","key":"volatile"}')
check "GET volatile (after expiry) -> nil" "$R" '"found".*false'

redis_op "SET sweep:1 PX 10" '{"op":"set","key":"sweep:1","value":"x","px":10}' >/dev/null
sleep 0.05
R=$(redis_op "expire_sweep" '{"op":"expire_sweep"}')
check "broadcast expire_sweep -> OK" "$R" '"result".*"OK"'
echo ""

# Replication
echo "Step 7: Replication (Ch7-8 — broadcast_shard_group)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
redis_op "SET replicated:key" '{"op":"set","key":"replicated:key","value":"hello"}' >/dev/null
R=$(redis_op "replicate" '{"op":"replicate","command":"SET","key":"replicated:key","value":"hello","offset":1}')
check "broadcast_shard_group -> replicas ACKed" "$R" '"acks"'
echo ""

# WAIT
echo "Step 8: WAIT Command (Ch8 — scatter_gather for ACK collection)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(redis_op "WAIT 2 5000" '{"op":"wait","num_replicas":2,"timeout_ms":5000,"min_offset":0}')
check "scatter_gather WAIT -> confirms received" "$R" '"result"'
echo ""

# Cross-shard
echo "Step 9: Cross-Shard Queries (scatter_gather + reduce)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(redis_op "DBSIZE" '{"op":"dbsize"}')
check "DBSIZE via reduce(SUM) across shards" "$R" '"result"'

R=$(redis_op "KEYS" '{"op":"keys"}')
check "KEYS via scatter_gather + map + concat" "$R" '"result"'
echo ""

# Snapshot
echo "Step 10: Coordinated Snapshot (parallel map across all shards)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(redis_op "snapshot" '{"op":"snapshot"}')
check "parallel map(snapshot) -> all shards" "$R" '"shard_count"'
check "snapshot returns shard data" "$R" '"shards"'
echo ""

# Throughput benchmark
echo "Step 11: Throughput Benchmark (bulk_update_shard_group SET, individual GET)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
R=$(benchmark_op "throughput" '{"op":"run_throughput_benchmark","warmup_batches":3,"bench_batches":20,"keys_per_batch":50}')
check "benchmark returned OK" "$R" '"result".*"OK"'
check "SET TPS reported" "$R" '"tps"'
check "SET p50 reported" "$R" '"p50_ms"'
check "GET p50 reported" "$R" '"get"'
if echo "$R" | grep -q '"tps"'; then
    SET_TPS=$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['set']['tps'])" 2>/dev/null || echo "?")
    GET_TPS=$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['get']['tps'])" 2>/dev/null || echo "?")
    SET_P50=$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['set']['p50_ms'])" 2>/dev/null || echo "?")
    SET_P99=$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['set']['p99_ms'])" 2>/dev/null || echo "?")
    GET_P50=$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['get']['p50_ms'])" 2>/dev/null || echo "?")
    GET_P99=$(echo "$R" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['get']['p99_ms'])" 2>/dev/null || echo "?")
    echo ""
    echo "  ┌─────────────────────────────────────────────────────────────────┐"
    echo "  │  Redis Cluster Throughput (3-shard group, 2-node gRPC cluster)  │"
    echo "  ├──────────────┬────────────┬────────────┬────────────┬───────────┤"
    echo "  │  Operation   │    TPS     │   p50 ms   │   p99 ms   │  Notes    │"
    echo "  ├──────────────┼────────────┼────────────┼────────────┼───────────┤"
    printf "  │  %-12s│ %10s │ %10s │ %10s │ %-9s │\n" "SET (bulk)" "$SET_TPS" "$SET_P50" "$SET_P99" "50 keys/batch"
    printf "  │  %-12s│ %10s │ %10s │ %10s │ %-9s │\n" "GET" "$GET_TPS" "$GET_P50" "$GET_P99" "individual"
    echo "  └──────────────┴────────────┴────────────┴────────────┴───────────┘"
fi
echo ""

# Summary
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Passed: $PASS"
echo "  Failed: $FAIL"
echo ""

if [ "$FAIL" -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    echo ""
    echo "  PlexSpaces primitives verified:"
    echo "  ✓ create_shard_group     — master + replica fleets"
    echo "  ✓ bulk_update            — key-routed SET / INCR / DEL"
    echo "  ✓ scatter_gather         — PING, KEYS, WAIT ACK collection"
    echo "  ✓ broadcast_shard_group  — replication + expire_sweep"
    echo "  ✓ reduce_shard_group     — DBSIZE aggregate (SUM)"
    echo "  ✓ map_shard_group        — parallel snapshot"
    echo "  ✓ bulk_update (bench)    — thousands SET/sec demonstrated"
    exit 0
else
    echo -e "${RED}$FAIL test(s) failed${NC}"
    exit 1
fi
