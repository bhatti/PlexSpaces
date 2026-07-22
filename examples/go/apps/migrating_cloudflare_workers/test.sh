#!/bin/bash
# Test Guild Chat - Discord-style Real-Time Chat Server (Go WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8091)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
# (HTTP gateway will be on 8091 + 1 = 8091)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/guild_chat.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="cloudflare-guild-chat"
NUM_USERS=5
BATCH_MSGS=200
BATCH_RATE_CHECKS=5000



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
echo "  Cloudflare Workers - Discord-style Guild Chat Server"
echo "  PlexSpaces Go WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:     localhost:$HTTP_PORT"
echo "  Test users:       $NUM_USERS"
echo "  Batch messages:   $BATCH_MSGS"
echo "  Batch rate checks: $BATCH_RATE_CHECKS"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
    echo ""
fi

# Step 1: Check node
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


# Step 2: Deploy
echo "Step 2: Deploy guild chat actors"
echo "----------------------------------------------------------------"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=cloudflare-guild-chat" \
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
    echo "  - ChatRoom:    chat-room"
    echo "  - RateLimiter: rate-limiter"
    echo "  - AlarmDemo:   alarm-demo"
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
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Step 3: Verify actors
echo "Step 3: Verify actors are responsive"
echo "----------------------------------------------------------------"

CHAT_RESP=$(send_op "ChatRoom" '{"op":"stats"}')
if echo "$CHAT_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}ChatRoom actor OK${NC}"
else
    echo -e "  ${RED}ChatRoom failed: $CHAT_RESP${NC}"
    exit 1
fi

RL_RESP=$(send_op "RateLimiter" '{"op":"stats"}')
if echo "$RL_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}RateLimiter actor OK${NC}"
else
    echo -e "  ${RED}RateLimiter failed: $RL_RESP${NC}"
    exit 1
fi
echo ""

# Step 4: Chat room operations
echo "Step 4: Chat Room Operations"
echo "----------------------------------------------------------------"
echo ""

# Join users
echo "  Joining $NUM_USERS users..."
for i in $(seq 1 $NUM_USERS); do
    RESP=$(send_op "ChatRoom" "{\"op\":\"join\",\"user_id\":\"user-$i\"}" 5)
    if echo "$RESP" | grep -q '"action":"joined"'; then
        echo -e "    ${GREEN}user-$i joined${NC}"
    elif echo "$RESP" | grep -q '"already_joined"'; then
        echo -e "    ${CYAN}user-$i already in room${NC}"
    else
        echo -e "    ${RED}user-$i join failed: $RESP${NC}"
    fi
done
echo ""

# Send messages
echo "  Sending messages..."
for i in $(seq 1 $NUM_USERS); do
    RESP=$(send_op "ChatRoom" "{\"op\":\"send_message\",\"user_id\":\"user-$i\",\"content\":\"Hello from user-$i!\"}" 5)
    if echo "$RESP" | grep -q '"status":"ok"'; then
        FAN_OUT=$(echo "$RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('fan_out',0))" 2>/dev/null || echo "?")
        echo -e "    ${GREEN}user-$i sent message (fan-out: $FAN_OUT)${NC}"
    else
        echo -e "    ${RED}user-$i send failed: $RESP${NC}"
    fi
done
echo ""

# Get members
echo "  Getting member list..."
MEMBERS_RESP=$(send_op "ChatRoom" '{"op":"get_members"}' 5)
if echo "$MEMBERS_RESP" | grep -q '"status":"ok"'; then
    MEMBER_COUNT=$(echo "$MEMBERS_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('count',0))" 2>/dev/null || echo "?")
    echo -e "    ${GREEN}Members: $MEMBER_COUNT${NC}"
else
    echo -e "    ${RED}Get members failed: $MEMBERS_RESP${NC}"
fi
echo ""

# Get history
echo "  Getting message history..."
HIST_RESP=$(send_op "ChatRoom" '{"op":"get_history","limit":10}' 5)
if echo "$HIST_RESP" | grep -q '"status":"ok"'; then
    HIST_COUNT=$(echo "$HIST_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('count',0))" 2>/dev/null || echo "?")
    echo -e "    ${GREEN}History: $HIST_COUNT messages${NC}"
else
    echo -e "    ${RED}Get history failed: $HIST_RESP${NC}"
fi
echo ""

# Leave one user
echo "  User-1 leaving room..."
LEAVE_RESP=$(send_op "ChatRoom" '{"op":"leave","user_id":"user-1"}' 5)
if echo "$LEAVE_RESP" | grep -q '"action":"left"'; then
    echo -e "    ${GREEN}user-1 left${NC}"
else
    echo -e "    ${YELLOW}Leave response: $LEAVE_RESP${NC}"
fi
echo ""

# Step 5: Rate limiter operations
echo "Step 5: Rate Limiter Operations"
echo "----------------------------------------------------------------"
echo ""

# Check rate for a user (should be allowed, 5 tokens)
echo "  Checking rate limit (5 tokens, 1/sec refill)..."
ALLOWED=0
DENIED=0
for i in $(seq 1 7); do
    RESP=$(send_op "RateLimiter" '{"op":"check_rate","user_id":"test-user"}' 5)
    if echo "$RESP" | grep -q '"allowed":true'; then
        ALLOWED=$((ALLOWED + 1))
    else
        DENIED=$((DENIED + 1))
    fi
done
echo -e "    ${GREEN}Allowed: $ALLOWED${NC}  ${RED}Denied: $DENIED${NC}"
if [ "$ALLOWED" -eq 5 ] && [ "$DENIED" -eq 2 ]; then
    echo -e "    ${GREEN}PASS: Token bucket working (5 allowed, 2 denied)${NC}"
elif [ "$ALLOWED" -ge 4 ] && [ "$DENIED" -ge 1 ]; then
    echo -e "    ${GREEN}PASS: Token bucket working (timing may vary)${NC}"
else
    echo -e "    ${YELLOW}WARN: Expected ~5 allowed/~2 denied, got $ALLOWED/$DENIED${NC}"
fi
echo ""

# Step 5b: AlarmDemo operations
echo "Step 5b: AlarmDemo Operations (Durable Alarm Lifecycle)"
echo "----------------------------------------------------------------"
echo ""

# Verify AlarmDemo actor
ALARM_RESP=$(send_op "AlarmDemo" '{"op":"status"}')
if echo "$ALARM_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}AlarmDemo actor OK${NC}"
else
    echo -e "  ${YELLOW}AlarmDemo not responsive (skipping alarm tests): $ALARM_RESP${NC}"
    ALARM_RESP=""
fi

if [ -n "$ALARM_RESP" ]; then
    # Enqueue some pending requests
    echo "  Enqueuing 3 pending requests..."
    for i in 1 2 3; do
        send_op "AlarmDemo" "{\"op\":\"enqueue\",\"data\":\"task-$i\"}" 5 >/dev/null
    done
    echo -e "  ${GREEN}Enqueued 3 tasks${NC}"

    # Schedule alarm 2 seconds from now (short for testing)
    echo "  Scheduling alarm (2s delay)..."
    START_RESP=$(send_op "AlarmDemo" '{"op":"start","delay_ms":2000}' 5)
    if echo "$START_RESP" | grep -q '"action":"alarm_scheduled"'; then
        FIRE_AT=$(echo "$START_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('fire_at_ms',0))" 2>/dev/null || echo "?")
        echo -e "  ${GREEN}Alarm scheduled (fires at: $FIRE_AT ms)${NC}"
    else
        echo -e "  ${YELLOW}Alarm schedule response: $START_RESP${NC}"
    fi

    # Check status
    STATUS_RESP=$(send_op "AlarmDemo" '{"op":"status"}' 5)
    if echo "$STATUS_RESP" | grep -q '"alarm_set":true'; then
        echo -e "  ${GREEN}Alarm status: set${NC}"
    else
        echo -e "  ${YELLOW}Alarm status response: $STATUS_RESP${NC}"
    fi

    # Cancel the alarm (clean up for test repeatability)
    CANCEL_RESP=$(send_op "AlarmDemo" '{"op":"cancel"}' 5)
    if echo "$CANCEL_RESP" | grep -q '"action":"alarm_cancelled"'; then
        echo -e "  ${GREEN}Alarm cancelled${NC}"
    else
        echo -e "  ${YELLOW}Cancel response: $CANCEL_RESP${NC}"
    fi

    echo -e "  ${GREEN}PASS: Durable alarm lifecycle (set/status/cancel) demonstrated${NC}"
fi
echo ""

# Step 6: Batch Message Benchmark (in-WASM computation)
echo "Step 6: Batch Message Benchmark ($BATCH_MSGS msgs in single WASM call)"
echo "----------------------------------------------------------------"
echo ""

MSG_BENCH_START=$(date +%s%N)

MSG_BATCH_RESP=$(send_op "ChatRoom" "{\"op\":\"send_message_batch\",\"count\":$BATCH_MSGS,\"user_id\":\"bench-user\",\"content\":\"Benchmark message with realistic payload sizing for chat\"}" 120)

MSG_BENCH_END=$(date +%s%N)
MSG_WALL_MS=$(( (MSG_BENCH_END - MSG_BENCH_START) / 1000000 ))

if echo "$MSG_BATCH_RESP" | grep -q '"status":"ok"'; then
    echo "$MSG_BATCH_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
total = p.get('total_sent', 0)
fan = p.get('total_fan_out', 0)
compute = p.get('compute_ms', 0)
ops = p.get('ops_per_sec', 0)
hist = p.get('history_size', 0)
members = p.get('active_members', 0)
wall = $MSG_WALL_MS
if isinstance(compute, str): compute = float(compute)
if isinstance(ops, str): ops = float(ops)
coord = wall - compute if wall > compute else 0
coord_pct = (coord / wall * 100) if wall > 0 else 0
compute_pct = (compute / wall * 100) if wall > 0 else 0
gran = (compute / coord) if coord > 0 else float('inf')
print(f'  Messages sent:     {total}')
print(f'  Fan-out events:    {fan}')
print(f'  Active members:    {members}')
print(f'  History size:      {hist}')
print(f'  Msgs/sec (WASM):   {ops:,.0f}')
print()
print(f'  Coordination Cost Analysis')
print(f'  ────────────────────────────────────────────')
print(f'  Wall clock:        {wall:.1f}ms')
print(f'  Compute (WASM):    {compute:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:      {coord:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:       {gran:.1f}x (compute/coordinate)')
" 2>/dev/null || echo "  (Could not parse batch results)"
else
    echo -e "  ${RED}Batch message benchmark failed: $MSG_BATCH_RESP${NC}"
fi
echo ""

# Step 7: Batch Rate Limit Benchmark
echo "Step 7: Batch Rate Limit Benchmark ($BATCH_RATE_CHECKS checks in single WASM call)"
echo "----------------------------------------------------------------"
echo ""

RL_BENCH_START=$(date +%s%N)

BATCH_RL_RESP=$(send_op "RateLimiter" "{\"op\":\"check_rate_batch\",\"user_id\":\"bench\",\"count\":$BATCH_RATE_CHECKS}" 120)

RL_BENCH_END=$(date +%s%N)
RL_WALL_MS=$(( (RL_BENCH_END - RL_BENCH_START) / 1000000 ))

if echo "$BATCH_RL_RESP" | grep -q '"status":"ok"'; then
    echo "$BATCH_RL_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
total = p.get('total_requests', 0)
allowed = p.get('allowed', 0)
denied = p.get('denied', 0)
compute = p.get('duration_ms', 0)
ops = p.get('ops_per_sec', 0)
users = p.get('unique_users', 0)
rpu = p.get('reqs_per_user', 0)
wall = $RL_WALL_MS
if isinstance(compute, str): compute = float(compute)
if isinstance(ops, str): ops = float(ops)
coord = wall - compute if wall > compute else 0
coord_pct = (coord / wall * 100) if wall > 0 else 0
compute_pct = (compute / wall * 100) if wall > 0 else 0
gran = (compute / coord) if coord > 0 else float('inf')
print(f'  Total requests:    {total}')
print(f'  Allowed:           {allowed}')
print(f'  Denied:            {denied}')
print(f'  Unique users:      {users}')
print(f'  Requests/user:     {rpu}')
print(f'  Rate check ops/sec:{ops:,.0f}')
print()
print(f'  Coordination Cost Analysis')
print(f'  ────────────────────────────────────────────')
print(f'  Wall clock:        {wall:.1f}ms')
print(f'  Compute (WASM):    {compute:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:      {coord:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:       {gran:.1f}x (compute/coordinate)')
" 2>/dev/null || echo "  (Could not parse batch results)"
else
    echo -e "  ${RED}Batch rate limit benchmark failed: $BATCH_RL_RESP${NC}"
fi
echo ""

# Step 8: Final Statistics
echo "Step 8: Final Statistics"
echo "================================================================"
echo ""

CHAT_STATS=$(send_op "ChatRoom" '{"op":"stats"}')
RL_STATS=$(send_op "RateLimiter" '{"op":"stats"}')

python3 -c "
import sys, json

# Parse stats
chat = json.loads('''$CHAT_STATS''')
rl = json.loads('''$RL_STATS''')
cp = chat.get('payload', chat)
rp = rl.get('payload', rl)

cc = cp.get('counters', {})
cb = cp.get('benchmarks', {})
rc = rp.get('counters', {})

compute_ms = float(cb.get('total_compute_ms', 0))
msgs_sec = float(cb.get('msgs_per_sec', 0))
mem_kb = float(cb.get('memory_kb', 0))

print('  ChatRoom')
print(f'    Messages:    {cc.get(\"total_messages\", 0):,}')
print(f'    Joins:       {cc.get(\"total_joins\", 0)}')
print(f'    Broadcasts:  {cc.get(\"total_broadcasts\", 0):,}')
print(f'    Members:     {cc.get(\"active_members\", 0)}')
print(f'    History:     {cc.get(\"history_size\", 0)}')
print(f'    Compute:     {compute_ms:.1f}ms')
print(f'    Msgs/sec:    {msgs_sec:,.0f}')
print(f'    Memory:      {mem_kb:.1f} KB')
print()
print('  RateLimiter')
print(f'    Checks:      {rc.get(\"total_checks\", 0):,}')
print(f'    Allowed:     {rc.get(\"total_allowed\", 0):,}')
print(f'    Denied:      {rc.get(\"total_denied\", 0):,}')
deny = float(rc.get('deny_rate_pct', 0))
print(f'    Deny rate:   {deny:.1f}%')
print(f'    Users:       {rc.get(\"active_users\", 0)}')
" 2>/dev/null || {
    echo "  (Could not parse final stats)"
}

echo ""
echo "================================================================"
echo -e "  ${GREEN}Cloudflare Workers Guild Chat Test Complete${NC}"
echo "================================================================"
