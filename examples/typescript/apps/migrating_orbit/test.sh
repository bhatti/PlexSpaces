#!/bin/bash
# Test Read State Tracker - Discord-style Read State Tracking (TypeScript WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8092)
#
# Prerequisites: Start PlexSpaces node first:
#   ./scripts/server.sh (from repo root)
# (HTTP gateway will be on 8092)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/read_state_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="orbit-read-state-ts"
NUM_USERS=10
NUM_CHANNELS=5
BATCH_UPDATES=500

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "================================================================"
echo "  Orbit - Discord-style Read State Tracker"
echo "  PlexSpaces TypeScript WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:     localhost:$HTTP_PORT"
echo "  Test users:       $NUM_USERS"
echo "  Channels/user:    $NUM_CHANNELS"
echo "  Batch updates:    $BATCH_UPDATES"
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
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "Start node first: ./scripts/server.sh (from repo root)"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""

trap cleanup EXIT

# Step 2: Deploy
echo "Step 2: Deploy read state tracker actor"
echo "----------------------------------------------------------------"

curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=orbit-read-state-ts" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}Deployed $APP_ID${NC}"
    echo "  - ReadStateTracker: read-state-tracker (virtual actor with durability)"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper
send_op() {
    local actor_id="$1"
    local payload="$2"
    local timeout="${3:-60}"
    # Generate a client-side message-id for logging (ULID-like format)
    local message_id="req-$(date +%s%N | cut -b1-13)$(openssl rand -hex 3 2>/dev/null || echo $(head -c 3 /dev/urandom | od -An -tx1 | tr -d ' \n'))"
    echo "  [CLIENT] Sending message: message_id=$message_id, actor_id=$actor_id, payload=$payload, timeout=${timeout}s" >&2
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor_id/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Step 3: Verify actor (virtual actor activates on first message)
echo "Step 3: Verify actor (virtual actor activates on first message)"
echo "----------------------------------------------------------------"

USER_ID="user-1"
ACTOR_ID="read-state-tracker:$USER_ID"
STATS_RESP=$(send_op "$ACTOR_ID" '{"op":"stats"}' 5)
if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}ReadStateTracker actor OK (virtual actor activated)${NC}"
else
    echo -e "  ${RED}ReadStateTracker failed: $STATS_RESP${NC}"
    exit 1
fi
echo ""

# Step 4: Mark read operations
echo "Step 4: Mark Read Operations"
echo "----------------------------------------------------------------"
echo ""

# Mark reads for multiple users and channels
echo "  Marking reads for $NUM_USERS users across $NUM_CHANNELS channels..."
for u in $(seq 1 $NUM_USERS); do
    USER_ID="user-$u"
    ACTOR_ID="read-state-tracker:$USER_ID"
    for c in $(seq 1 $NUM_CHANNELS); do
        CHANNEL_ID="channel-$c"
        MESSAGE_ID="msg-$c-$(date +%s%N | cut -b1-13)"
        # Generate timestamp in milliseconds (ensure it's a valid number)
        TIMESTAMP=$(date +%s%3N 2>/dev/null || echo "$(date +%s)000")
        # Remove any non-numeric characters (e.g., 'N' suffix on some systems)
        TIMESTAMP=$(echo "$TIMESTAMP" | tr -d '[:alpha:]')
        
        RESP=$(send_op "$ACTOR_ID" "{\"op\":\"mark_read\",\"channel_id\":\"$CHANNEL_ID\",\"message_id\":\"$MESSAGE_ID\",\"timestamp\":$TIMESTAMP}" 5)
        if echo "$RESP" | grep -q '"status":"ok"'; then
            if [ "$u" -le 3 ] && [ "$c" -le 2 ]; then
                echo -e "    ${GREEN}user-$u: channel-$c marked as read${NC}"
            fi
        else
            echo -e "    ${RED}user-$u: channel-$c failed: $RESP${NC}"
        fi
    done
done
echo ""

# Step 5: Get read states
echo "Step 5: Get Read States"
echo "----------------------------------------------------------------"
echo ""

# Get read state for a specific channel
USER_ID="user-1"
ACTOR_ID="read-state-tracker:$USER_ID"
CHANNEL_ID="channel-1"
GET_RESP=$(send_op "$ACTOR_ID" "{\"op\":\"get_read_state\",\"channel_id\":\"$CHANNEL_ID\"}" 5)
if echo "$GET_RESP" | grep -q '"status":"ok"'; then
    MESSAGE_ID=$(echo "$GET_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); rs=p.get('read_state',{}); print(rs.get('last_read_message_id','N/A'))" 2>/dev/null || echo "N/A")
    echo -e "  ${GREEN}user-1 channel-1 last read: $MESSAGE_ID${NC}"
else
    echo -e "  ${RED}Get read state failed: $GET_RESP${NC}"
fi
echo ""

# Get all read states for a user
GET_ALL_RESP=$(send_op "$ACTOR_ID" '{"op":"get_all_read_states"}' 5)
if echo "$GET_ALL_RESP" | grep -q '"status":"ok"'; then
    CHANNEL_COUNT=$(echo "$GET_ALL_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(len(p.get('channels',{})))" 2>/dev/null || echo "0")
    echo -e "  ${GREEN}user-1 total channels tracked: $CHANNEL_COUNT${NC}"
else
    echo -e "  ${RED}Get all read states failed: $GET_ALL_RESP${NC}"
fi
echo ""

# Step 6: Batch Update Benchmark
echo "Step 6: Batch Update Benchmark ($BATCH_UPDATES updates in single WASM call)"
echo "----------------------------------------------------------------"
echo ""

BATCH_START=$(date +%s%N)

# Prepare batch updates
BATCH_UPDATES_JSON="["
for i in $(seq 1 $BATCH_UPDATES); do
    CHANNEL_ID="channel-$((i % NUM_CHANNELS + 1))"
    MESSAGE_ID="msg-batch-$i-$(date +%s%N | cut -b1-13)"
    # Generate timestamp in milliseconds (ensure it's a valid number)
    TIMESTAMP=$(date +%s%3N 2>/dev/null || echo "$(date +%s)000")
    # Remove any non-numeric characters (e.g., 'N' suffix on some systems)
    TIMESTAMP=$(echo "$TIMESTAMP" | tr -d '[:alpha:]')
    if [ "$i" -gt 1 ]; then
        BATCH_UPDATES_JSON="$BATCH_UPDATES_JSON,"
    fi
    BATCH_UPDATES_JSON="$BATCH_UPDATES_JSON{\"channel_id\":\"$CHANNEL_ID\",\"message_id\":\"$MESSAGE_ID\",\"timestamp\":$TIMESTAMP}"
done
BATCH_UPDATES_JSON="$BATCH_UPDATES_JSON]"

USER_ID="user-batch"
ACTOR_ID="read-state-tracker:$USER_ID"
BATCH_RESP=$(send_op "$ACTOR_ID" "{\"op\":\"batch_mark_read\",\"updates\":$BATCH_UPDATES_JSON}" 120)

BATCH_END=$(date +%s%N)
BATCH_WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))

if echo "$BATCH_RESP" | grep -q '"status":"ok"'; then
    echo "$BATCH_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
total = p.get('total_updates', 0)
updated = p.get('channels_updated', 0)
created = p.get('channels_created', 0)
channels = p.get('total_channels', 0)
compute = p.get('compute_ms', 0)
ops = p.get('ops_per_sec', 0)
wall = $BATCH_WALL_MS
if isinstance(compute, str): compute = float(compute)
if isinstance(ops, str): ops = float(ops)
coord = wall - compute if wall > compute else 0
coord_pct = (coord / wall * 100) if wall > 0 else 0
compute_pct = (compute / wall * 100) if wall > 0 else 0
gran = (compute / coord) if coord > 0 else float('inf')
print(f'  Total updates:     {total}')
print(f'  Channels updated:  {updated}')
print(f'  Channels created:  {created}')
print(f'  Total channels:    {channels}')
print(f'  Updates/sec (WASM): {ops:,.0f}')
print()
print(f'  Coordination Cost Analysis')
print(f'  ────────────────────────────────────────────')
print(f'  Wall clock:        {wall:.1f}ms')
print(f'  Compute (WASM):    {compute:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:      {coord:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:       {gran:.1f}x (compute/coordinate)')
" 2>/dev/null || echo "  (Could not parse batch results)"
else
    echo -e "  ${RED}Batch update benchmark failed: $BATCH_RESP${NC}"
fi
echo ""

# Step 7: Virtual Actor Lifecycle Test
echo "Step 7: Virtual Actor Lifecycle Test (deactivation/reactivation)"
echo "----------------------------------------------------------------"
echo ""

# Get stats before (actor is active)
USER_ID="user-lifecycle"
ACTOR_ID="read-state-tracker:$USER_ID"
BEFORE_RESP=$(send_op "$ACTOR_ID" '{"op":"stats"}' 5)
BEFORE_UPDATES=$(echo "$BEFORE_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('total_updates',0))" 2>/dev/null || echo "0")

# Mark a read (activates actor)
# Generate timestamp and ensure it's a valid number
TEST_TIMESTAMP=$(date +%s%3N 2>/dev/null || echo "$(date +%s)000")
TEST_TIMESTAMP=$(echo "$TEST_TIMESTAMP" | tr -d '[:alpha:]')
send_op "$ACTOR_ID" "{\"op\":\"mark_read\",\"channel_id\":\"test-channel\",\"message_id\":\"test-msg-1\",\"timestamp\":$TEST_TIMESTAMP}" 5 > /dev/null

# Wait a bit (virtual actor may deactivate after idle_timeout)
sleep 2

# Get stats after (actor reactivates if deactivated)
AFTER_RESP=$(send_op "$ACTOR_ID" '{"op":"stats"}' 5)
AFTER_UPDATES=$(echo "$AFTER_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('total_updates',0))" 2>/dev/null || echo "0")

if [ "$AFTER_UPDATES" -ge "$BEFORE_UPDATES" ]; then
    echo -e "  ${GREEN}Virtual actor lifecycle OK (state persisted: $AFTER_UPDATES updates)${NC}"
else
    echo -e "  ${YELLOW}WARN: Updates may have been lost ($BEFORE_UPDATES -> $AFTER_UPDATES)${NC}"
fi
echo ""

# Step 8: Final Statistics
echo "Step 8: Final Statistics"
echo "================================================================"
echo ""

# Get stats from multiple users
TOTAL_CHANNELS=0
TOTAL_UPDATES=0
for u in $(seq 1 $NUM_USERS); do
    USER_ID="user-$u"
    ACTOR_ID="read-state-tracker:$USER_ID"
    STATS_RESP=$(send_op "$ACTOR_ID" '{"op":"stats"}' 5)
    if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
        CHANNELS=$(echo "$STATS_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('total_channels',0))" 2>/dev/null || echo "0")
        UPDATES=$(echo "$STATS_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('total_updates',0))" 2>/dev/null || echo "0")
        TOTAL_CHANNELS=$((TOTAL_CHANNELS + CHANNELS))
        TOTAL_UPDATES=$((TOTAL_UPDATES + UPDATES))
    fi
done

python3 -c "
import sys
users = $NUM_USERS
channels = $TOTAL_CHANNELS
updates = $TOTAL_UPDATES
print(f'  Total users:        {users}')
print(f'  Total channels:    {channels}')
print(f'  Total updates:      {updates:,}')
print(f'  Avg channels/user: {channels/users if users > 0 else 0:.1f}')
print(f'  Avg updates/user:  {updates/users if users > 0 else 0:.1f}')
" 2>/dev/null || {
    echo "  (Could not parse final stats)"
}

echo ""
echo "================================================================"
echo -e "  ${GREEN}Orbit Read State Tracker (TypeScript) Test Complete${NC}"
echo "================================================================"
