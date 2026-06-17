#!/bin/bash
# Test Game Matchmaking - Virtual Actor per Game Room (Go WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8091)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
# (HTTP gateway will be on 8091 + 1 = 8091)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/matchmaking.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="rivet-matchmaking"
ACTOR_TYPE="game-room"  # Actor type from app-config.toml
NUM_ROOMS=100
PLAYERS_PER_ROOM=10



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
echo "  Rivet Game Matchmaking - Virtual Actor per Room"
echo "  PlexSpaces Go WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:   localhost:$HTTP_PORT"
echo "  Rooms:          $NUM_ROOMS"
echo "  Players/room:   $PLAYERS_PER_ROOM"
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
echo "Step 2: Deploy matchmaking actor"
echo "----------------------------------------------------------------"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=rivet-matchmaking" \
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
    echo "  - Game room: game-room"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper
# For virtual actors, URL format is: /api/v1/actors/{namespace}/{actor_type}:{instance_id}
send_op() {
    local room_id="$1"  # instance_id (e.g., "room-1")
    local payload="$2"
    local timeout="${3:-60}"
    # Format: /api/v1/actors/{namespace}/{actor_type}:{instance_id}
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$room_id/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Test single room
echo "Step 3: Single Room Test"
echo "----------------------------------------------------------------"

ROOM_ID="room-1"
echo "  Creating room: $ROOM_ID"

# Join players
for i in $(seq 1 5); do
    PLAYER_ID="player-$i"
    SKILL=$((30 + i * 10))
    RESP=$(send_op "$ROOM_ID" "{\"op\":\"join\",\"player_id\":\"$PLAYER_ID\",\"skill_level\":$SKILL,\"region\":\"us-east\"}" 5)
    if echo "$RESP" | grep -q '"status":"ok"'; then
        echo -e "    ${GREEN}Joined: $PLAYER_ID (skill: $SKILL)${NC}"
    else
        echo -e "    ${RED}Join failed: $RESP${NC}"
    fi
done
echo ""

# Get status
echo "  Getting room status..."
STATUS_RESP=$(send_op "$ROOM_ID" '{"op":"get_status"}' 5)
if echo "$STATUS_RESP" | grep -q '"status":"ok"'; then
    echo -e "    ${GREEN}Room status retrieved${NC}"
    echo "$STATUS_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', {})
print(f'    Players: {p.get(\"players_count\", 0)}/{p.get(\"max_players\", 0)}')
print(f'    Status: {p.get(\"status\", \"unknown\")}')
" 2>/dev/null || echo "    (Could not parse status)"
else
    echo -e "    ${RED}Status failed: $STATUS_RESP${NC}"
fi
echo ""

# Matchmake
echo "  Running matchmaking..."
MATCH_RESP=$(send_op "$ROOM_ID" '{"op":"matchmake"}' 5)
if echo "$MATCH_RESP" | grep -q '"matched":true'; then
    echo -e "    ${GREEN}Matchmaking successful${NC}"
    echo "$MATCH_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
teams = d.get('teams', [])
print(f'    Matched teams: {len(teams)}')
for i, team in enumerate(teams):
    print(f'      Team {i+1}: {len(team)} players')
" 2>/dev/null || echo "    (Could not parse match result)"
else
    echo -e "    ${YELLOW}No matches found${NC}"
fi
echo ""

# Batch test
echo "Step 4: Batch Test ($NUM_ROOMS rooms, $PLAYERS_PER_ROOM players each)"
echo "----------------------------------------------------------------"
echo ""

BATCH_START=$(date +%s%N)

# Create rooms and join players
for room_num in $(seq 1 $NUM_ROOMS); do
    ROOM_ID="room-$room_num"
    
    # Join players to room
    for player_num in $(seq 1 $PLAYERS_PER_ROOM); do
        PLAYER_ID="player-$room_num-$player_num"
        SKILL=$((20 + (player_num % 50)))
        REGION="us-east"
        if [ $((room_num % 3)) -eq 0 ]; then
            REGION="eu-west"
        fi
        
        send_op "$ROOM_ID" "{\"op\":\"join\",\"player_id\":\"$PLAYER_ID\",\"skill_level\":$SKILL,\"region\":\"$REGION\"}" 5 >/dev/null 2>&1 || true
    done
    
    # Try matchmaking
    send_op "$ROOM_ID" '{"op":"matchmake"}' 5 >/dev/null 2>&1 || true
    
    if [ $((room_num % 20)) -eq 0 ]; then
        echo "  Processed $room_num/$NUM_ROOMS rooms..."
    fi
done

BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))

echo ""
echo -e "${GREEN}Batch test complete${NC}"
echo "  Wall clock time: ${WALL_MS}ms"
echo ""

# Get stats from the last room used in the batch (most likely still active with state)
echo "Step 5: Statistics & Benchmarks"
echo "================================================================"
echo ""

STATS_ROOM="room-$NUM_ROOMS"
STATS_RESP=$(send_op "$STATS_ROOM" '{"op":"get_stats"}' 5)
# When stats are empty, Python will write raw response here for diagnosis
export STATS_DEBUG_FILE="${SCRIPT_DIR}/.stats_response_debug.json"

if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo "$STATS_RESP" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
# HTTP gateway: { \"payload\": <actor_response> }; actor: { \"status\": \"ok\", \"payload\": { config, counters, benchmarks } }
inner = d.get('payload', d)
if isinstance(inner, str):
    try: inner = json.loads(inner)
    except Exception: inner = {}
p = inner.get('payload', inner) if isinstance(inner, dict) else inner
if not isinstance(p, dict):
    p = {}
config = p.get('config', {})
counters = p.get('counters', {})
bench = p.get('benchmarks', {})

# When stats look empty, write full response for diagnosis
debug_file = os.environ.get('STATS_DEBUG_FILE', '')
if debug_file and (config.get('room_id') in (None, 'unknown') or counters.get('total_joins', 0) == 0):
    try:
        with open(debug_file, 'w') as f:
            f.write(raw)
        print('  (Stats empty; raw response saved to .stats_response_debug.json for debugging)', file=sys.stderr)
    except Exception:
        pass

print('  Room Configuration')
print('  ────────────────────────────────────────────')
print(f'  Room ID:             {config.get(\"room_id\", \"unknown\")}')
print(f'  Max players:         {config.get(\"max_players\", 0)}')
print(f'  Min players:         {config.get(\"min_players\", 0)}')
print(f'  Game mode:           {config.get(\"game_mode\", \"unknown\")}')
print(f'  Idle timeout:        {config.get(\"idle_timeout_ms\", 0)}ms')
print()
print('  Counters')
print('  ────────────────────────────────────────────')
print(f'  Total joins:         {counters.get(\"total_joins\", 0):,}')
print(f'  Total matches:       {counters.get(\"total_matches\", 0):,}')
print(f'  Total deactivations: {counters.get(\"total_deactivations\", 0):,}')
print(f'  Current players:     {counters.get(\"current_players\", 0)}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
total_ms = bench.get('total_ms', 0)
compute_ms = bench.get('compute_ms', 0)
coord_ms = bench.get('coord_ms', 0)
compute_pct = bench.get('compute_pct', 0)
coord_pct = bench.get('coord_pct', 0)
gran = bench.get('granularity', 0)
if isinstance(total_ms, str): total_ms = float(total_ms)
if isinstance(compute_ms, str): compute_ms = float(compute_ms)
if isinstance(coord_ms, str): coord_ms = float(coord_ms)
if isinstance(compute_pct, str): compute_pct = float(compute_pct)
if isinstance(coord_pct, str): coord_pct = float(coord_pct)
if isinstance(gran, str): gran = float(gran)
print(f'  Total time:          {total_ms:.1f}ms')
print(f'  Compute time:        {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination time:   {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:         {gran:.1f}x (compute/coordinate)')
" 2>/dev/null || echo "  (Could not parse stats)"
else
    echo -e "  ${RED}Stats failed: $STATS_RESP${NC}"
fi

echo ""
echo -e "${BOLD}Wall clock time: ${WALL_MS}ms${NC}"
echo ""
echo "================================================================"
echo -e "  ${GREEN}Rivet Game Matchmaking Test Complete${NC}"
echo "================================================================"
