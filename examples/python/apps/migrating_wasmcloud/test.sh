#!/bin/bash
# Test Session Store Service - wasmCloud-style Capability-Based Design (Python WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8091)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
# (HTTP gateway will be on 8091 + 1 = 8091)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/session_store.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="session-store"
NUM_SESSIONS=10
NUM_OPERATIONS=50


echo "================================================================"
echo "  Session Store Service - wasmCloud Capability-Based Design"
echo "  PlexSpaces Python WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:   localhost:$HTTP_PORT"
echo "  Sessions:       $NUM_SESSIONS"
echo "  Operations:     $NUM_OPERATIONS"
echo "  Default TTL:    3600s (1 hour)"
echo "  Cleanup:        Every 60s"
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


# Deploy
echo "Step 2: Deploy session store actor"
echo "----------------------------------------------------------------"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=session-store" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
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
    echo "  - Session store: session-store"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper to send POST and get response
# Usage: send_op <actor> <payload> [timeout_secs]
send_op() {
    local actor="$1"
    local payload="$2"
    local timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Verify actor is alive
echo "Step 3: Verify session store actor"
echo "----------------------------------------------------------------"

STATS_RESP=$(send_op "session-store" '{"op":"stats"}')
if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}Session store actor OK${NC}"
else
    echo -e "  ${RED}Session store failed: $STATS_RESP${NC}"
    exit 1
fi
echo ""

# Create sessions
echo "Step 4: Create $NUM_SESSIONS Sessions (wasmCloud KeyValue capability)"
echo "----------------------------------------------------------------"

CREATE_START=$(date +%s%N)
CREATED=0
FAILED=0
SESSION_IDS_FILE=$(mktemp)

for i in $(seq 1 $NUM_SESSIONS); do
    user_id="user-$(printf "%04d" $i)"
    RESP=$(send_op "session-store" "{\"op\":\"create\",\"user_id\":\"$user_id\",\"ttl_sec\":3600}" 5)
    
    if echo "$RESP" | grep -q '"status":"ok"'; then
        CREATED=$((CREATED + 1))
        # Extract session_id from response
        session_id=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('session_id',''))" 2>/dev/null || echo "")
        if [ -n "$session_id" ]; then
            echo "$session_id" >> "$SESSION_IDS_FILE"
        fi
    else
        FAILED=$((FAILED + 1))
    fi
    
    if [ $((i % 50)) -eq 0 ]; then
        echo "  Created $i/$NUM_SESSIONS sessions..."
    fi
done

CREATE_END=$(date +%s%N)
CREATE_MS=$(( (CREATE_END - CREATE_START) / 1000000 ))

echo ""
echo "  Created: $CREATED sessions"
echo "  Failed:  $FAILED sessions"
echo "  Time:    ${CREATE_MS}ms"
if [ $CREATE_MS -gt 0 ]; then
    echo "  Rate:    $((CREATED * 1000 / CREATE_MS)) sessions/sec"
else
    echo "  Rate:    N/A"
fi
echo ""

# Access sessions (read operations)
echo "Step 5: Access Sessions ($NUM_OPERATIONS operations)"
echo "----------------------------------------------------------------"

# Load session IDs into array
SESSION_COUNT=$(wc -l < "$SESSION_IDS_FILE" 2>/dev/null || echo "0")
SESSION_ARRAY=()
if [ "$SESSION_COUNT" -gt 0 ]; then
    while IFS= read -r line; do
        SESSION_ARRAY+=("$line")
    done < "$SESSION_IDS_FILE"
fi

ACCESS_START=$(date +%s%N)
ACCESSED=0
NOT_FOUND=0
EXPIRED=0

# Use stored session IDs (cycle through them)
if [ "$SESSION_COUNT" -eq 0 ]; then
    echo "  ${YELLOW}No session IDs available, skipping access test${NC}"
else
    for i in $(seq 1 $NUM_OPERATIONS); do
        idx=$(( (i - 1) % SESSION_COUNT ))
        session_id="${SESSION_ARRAY[$idx]}"
        
        # Try to get session
        RESP=$(send_op "session-store" "{\"op\":\"get\",\"session_id\":\"$session_id\"}" 5)
        
        if echo "$RESP" | grep -q '"status":"ok"'; then
            ACCESSED=$((ACCESSED + 1))
        elif echo "$RESP" | grep -q '"session not found"'; then
            NOT_FOUND=$((NOT_FOUND + 1))
        elif echo "$RESP" | grep -q '"session expired"'; then
            EXPIRED=$((EXPIRED + 1))
        fi
        
        if [ $((i % 200)) -eq 0 ]; then
            echo "  Processed $i/$NUM_OPERATIONS operations..."
        fi
    done
fi

ACCESS_END=$(date +%s%N)
ACCESS_MS=$(( (ACCESS_END - ACCESS_START) / 1000000 ))

echo ""
echo "  Accessed:    $ACCESSED sessions"
echo "  Not found:   $NOT_FOUND"
echo "  Expired:     $EXPIRED"
echo "  Time:        ${ACCESS_MS}ms"
if [ $ACCESS_MS -gt 0 ]; then
    echo "  Rate:        $((NUM_OPERATIONS * 1000 / ACCESS_MS)) ops/sec"
else
    echo "  Rate:        N/A"
fi
echo ""

# Refresh sessions
echo "Step 6: Refresh Sessions (extend TTL)"
echo "----------------------------------------------------------------"

REFRESH_START=$(date +%s%N)
REFRESHED=0
REFRESH_COUNT=50

if [ "$SESSION_COUNT" -eq 0 ]; then
    echo "  ${YELLOW}No session IDs available, skipping refresh test${NC}"
else
    # Refresh first REFRESH_COUNT sessions
    for i in $(seq 0 $((REFRESH_COUNT - 1))); do
        if [ $i -ge "$SESSION_COUNT" ]; then
            break
        fi
        session_id="${SESSION_ARRAY[$i]}"
        
        RESP=$(send_op "session-store" "{\"op\":\"refresh\",\"session_id\":\"$session_id\",\"extend_ttl_sec\":3600}" 5)
        
        if echo "$RESP" | grep -q '"status":"ok"'; then
            REFRESHED=$((REFRESHED + 1))
        fi
    done
fi

REFRESH_END=$(date +%s%N)
REFRESH_MS=$(( (REFRESH_END - REFRESH_START) / 1000000 ))

echo "  Refreshed: $REFRESHED sessions"
echo "  Time:      ${REFRESH_MS}ms"
if [ $REFRESH_MS -gt 0 ]; then
    echo "  Rate:      $((REFRESHED * 1000 / REFRESH_MS)) ops/sec"
else
    echo "  Rate:      N/A"
fi
echo ""

# Cleanup temp file
rm -f "$SESSION_IDS_FILE"

# Trigger cleanup manually
echo "Step 7: Trigger Cleanup (wasmCloud Timer capability)"
echo "----------------------------------------------------------------"

CLEANUP_RESP=$(send_op "session-store" '{"op":"cleanup_expired"}' 30)

if echo "$CLEANUP_RESP" | grep -q '"status":"ok"'; then
    CHECKED=$(echo "$CLEANUP_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('checked',0))" 2>/dev/null || echo "?")
    EXPIRED=$(echo "$CLEANUP_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('expired',0))" 2>/dev/null || echo "?")
    echo "  Checked:  $CHECKED sessions"
    echo "  Expired:  $EXPIRED sessions"
else
    echo -e "  ${YELLOW}Cleanup: $CLEANUP_RESP${NC}"
fi
echo ""

# Final stats with benchmarks
echo "Step 8: Final Statistics & Benchmarks"
echo "----------------------------------------------------------------"

STATS_RESP=$(send_op "session-store" '{"op":"stats"}')

if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo "$STATS_RESP" | python3 -c "
import sys, json

raw = json.load(sys.stdin)
d = raw.get('payload', raw)  # unwrap HTTP envelope
sessions = d.get('sessions', {})
validation = d.get('validation', {})
config = d.get('config', {})

print()
print('  Sessions')
print('  --------')
print(f'  Active:           {sessions.get(\"active\", 0):,}')
print(f'  Total created:     {sessions.get(\"total_created\", 0):,}')
print(f'  Total accessed:   {sessions.get(\"total_accessed\", 0):,}')
print(f'  Total refreshed:  {sessions.get(\"total_refreshed\", 0):,}')
print(f'  Total deleted:    {sessions.get(\"total_deleted\", 0):,}')
print(f'  Total expired:    {sessions.get(\"total_expired\", 0):,}')
print()
print('  Validation')
print('  ----------')
print(f'  Total validations: {validation.get(\"total_validations\", 0):,}')
print(f'  Failures:          {validation.get(\"total_failures\", 0):,}')
print(f'  Success rate:      {validation.get(\"success_rate\", 0):.1f}%')
print()
print('  Configuration')
print('  -------------')
print(f'  Default TTL:       {config.get(\"default_ttl_sec\", 0)}s')
print(f'  Cleanup interval:  {config.get(\"cleanup_interval_sec\", 0)}s')
print(f'  Auth service:      {config.get(\"auth_service\", \"disabled\")}')
print()
" 2>/dev/null || echo "  (could not parse stats)"
else
    echo -e "  ${YELLOW}Stats: $STATS_RESP${NC}"
fi

echo "================================================================"
echo ""
echo "  This example demonstrated:"
echo "    - wasmCloud-style capability-based design"
echo "    - KeyValue capability (distributed session storage)"
echo "    - Timer capability (periodic cleanup)"
echo "    - Inter-actor communication (simulates HTTP capability)"
echo "    - Session CRUD operations (create, get, refresh, delete)"
echo ""
echo "  wasmCloud equivalent: WASM actor with HTTP + KeyValue + Timer capabilities"
echo "  PlexSpaces:          @actor + host.kv_* + host.send_after() + host.ask()"
echo ""
echo "================================================================"
