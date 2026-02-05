#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test Leader Election - Distributed lock for leader election
#
# Two-terminal test:
# - Terminal 1: Run ./test.sh (or LEADER_ELECTION_OWNER_ID=term1 ./test.sh). Deploys if needed,
#   acquires lock, renews lease every 15s for 60s, then releases and exits.
# - Terminal 2: Run LEADER_ELECTION_OWNER_ID=term2 ./test.sh. Deploys if needed, then keeps
#   trying to acquire the lock for up to 120s (shows "Acquiring lock..."). Kill Terminal 1;
#   after lease expiry (~30s) Terminal 2 acquires, renews for 60s, then exits.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/leader_election_actor.wasm"
HTTP_PORT="${1:-8092}"

# Leader holds lock and renews for this long (seconds)
RENEW_DURATION=60
# Renew lease every this many seconds while leader
RENEW_INTERVAL=15
# Non-leader keeps trying to acquire for up to this long (seconds)
ACQUIRE_TIMEOUT=120
# Poll interval when waiting for lock (seconds)
ACQUIRE_POLL=5

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Use env so two terminals can set term1/term2; otherwise unique per run
OWNER_ID="${LEADER_ELECTION_OWNER_ID:-owner-$$-${RANDOM}}"
APP_ID="leader-election-${OWNER_ID}"
ACTOR_NAME="LeaderElection"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Leader Election - Distributed Lock Example            ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "  ${GREEN}Owner ID: ${OWNER_ID}${NC}"
echo -e "${YELLOW}💡 Two-terminal: Terminal 1 = leader (renews 60s then exits). Terminal 2 = acquires lock for up to 120s until you kill Terminal 1.${NC}"
echo ""

source ~/venv/bin/activate 2>/dev/null || true

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Step 1: Check node
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Payload with owner id for display
PAYLOAD_BASE="{\"msg_type\":\"%s\",\"payload\":{\"candidate_id\":\"$OWNER_ID\"}}"

# Invoke actor and return 0 if app is deployed and responding (HTTP 200 and body contains leader key)
# Response may be {"success":true,"payload":{"leader":true,...}} or payload as string
check_app_deployed() {
    local payload
    payload=$(printf "$PAYLOAD_BASE" "status")
    local code
    code=$(curl -s -o /tmp/leader_election_status.$$ -w "%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" -d "$payload" --max-time 10 2>/dev/null) || code="000"
    if [ "$code" = "200" ] && grep -q 'leader' /tmp/leader_election_status.$$ 2>/dev/null; then
        rm -f /tmp/leader_election_status.$$
        return 0
    fi
    rm -f /tmp/leader_election_status.$$
    return 1
}

# Step 2: Deploy only if app not already deployed
echo "Step 2: Deploy leader election actor (skip if already deployed)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT

if check_app_deployed; then
    echo -e "${GREEN}✓ App already deployed ($APP_ID), skipping deploy${NC}"
else
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
        -F "application_id=$APP_ID" \
        -F "name=$ACTOR_NAME" \
        -F "version=1.0.0" \
        -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true
    if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
        echo -e "${GREEN}✓ Deployed leader election actor${NC}"
    else
        echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
        exit 1
    fi
    sleep 2
fi
echo ""

# Try to become leader. Response may have "leader":true or \"leader\":true (nested payload).
try_acquire() {
    local payload
    payload=$(printf "$PAYLOAD_BASE" "try_lead")
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" -d "$payload" 2>/dev/null) || RESPONSE=""
    if echo "$RESPONSE" | grep -qE '"leader"\s*:\s*true|\\"leader\\":\s*true'; then
        return 0
    fi
    return 1
}

# Renew lease once. Response may have "renewed":true in payload.
do_renew() {
    local payload
    payload=$(printf "$PAYLOAD_BASE" "renew_lead")
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" -d "$payload" 2>/dev/null) || RESPONSE=""
    if echo "$RESPONSE" | grep -qE '"renewed"\s*:\s*true|\\"renewed\\":\s*true'; then
        return 0
    fi
    return 1
}

# Release leadership
do_release() {
    local payload
    payload=$(printf "$PAYLOAD_BASE" "release_lead")
    curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" -d "$payload" >/dev/null 2>&1 || true
}

echo "Step 3: Try to acquire leader lock"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# After deploy the actor may not be ready immediately; retry a few times so first terminal becomes leader
ACQUIRE_RETRIES=6
ACQUIRE_RETRY_SLEEP=3
got_leader=false
retry=1
while [ $retry -le "$ACQUIRE_RETRIES" ]; do
    if try_acquire; then
        got_leader=true
        break
    fi
    [ $retry -lt "$ACQUIRE_RETRIES" ] && sleep "$ACQUIRE_RETRY_SLEEP"
    retry=$((retry + 1))
done

if [ "$got_leader" = true ]; then
    echo -e "  ${GREEN}✓ Acquired lock → leader ($OWNER_ID)${NC}"
    echo ""
    echo "Step 4: Renewing lease every ${RENEW_INTERVAL}s for ${RENEW_DURATION}s (leader holds lock)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    elapsed=0
    while [ $elapsed -lt "$RENEW_DURATION" ]; do
        sleep "$RENEW_INTERVAL"
        elapsed=$((elapsed + RENEW_INTERVAL))
        if do_renew; then
            echo -e "  ${GREEN}✓${NC} Renewed lease at ${elapsed}s"
        else
            echo -e "  ${YELLOW}⚠${NC} Renew failed at ${elapsed}s"
        fi
    done
    echo -e "${GREEN}✓ Held lock for ${RENEW_DURATION}s${NC}"
    echo ""
    echo "Step 5: Release leadership"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    do_release
    echo -e "  ${GREEN}✓ Released ($OWNER_ID)${NC}"
    echo ""
    echo -e "${GREEN}✅ Leader Election Test Complete (was leader)${NC}"
    exit 0
fi

# Not leader: keep trying to acquire for up to ACQUIRE_TIMEOUT seconds
echo -e "  ${YELLOW}○${NC} Not leader ($OWNER_ID) — lock held by another. Will try to acquire for up to ${ACQUIRE_TIMEOUT}s."
echo ""
echo "Step 4: Acquiring lock (up to ${ACQUIRE_TIMEOUT}s — kill the other terminal to release)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
elapsed=0
while [ $elapsed -lt "$ACQUIRE_TIMEOUT" ]; do
    echo -e "  ${YELLOW}Acquiring lock...${NC} (${elapsed}s / ${ACQUIRE_TIMEOUT}s)"
    sleep "$ACQUIRE_POLL"
    elapsed=$((elapsed + ACQUIRE_POLL))
    if try_acquire; then
        echo -e "  ${GREEN}✓ Acquired lock → leader ($OWNER_ID)${NC}"
        echo ""
        echo "Step 5: Renewing lease every ${RENEW_INTERVAL}s for ${RENEW_DURATION}s"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        held=0
        while [ $held -lt "$RENEW_DURATION" ]; do
            sleep "$RENEW_INTERVAL"
            held=$((held + RENEW_INTERVAL))
            if do_renew; then
                echo -e "  ${GREEN}✓${NC} Renewed lease at ${held}s"
            else
                echo -e "  ${YELLOW}⚠${NC} Renew failed at ${held}s"
            fi
        done
        echo ""
        echo "Step 6: Release leadership"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        do_release
        echo -e "  ${GREEN}✓ Released ($OWNER_ID)${NC}"
        echo ""
        echo -e "${GREEN}✅ Leader Election Test Complete (acquired after ${elapsed}s)${NC}"
        exit 0
    fi
done

echo -e "${RED}✗ Timeout: could not acquire lock within ${ACQUIRE_TIMEOUT}s${NC}"
echo ""
exit 1
