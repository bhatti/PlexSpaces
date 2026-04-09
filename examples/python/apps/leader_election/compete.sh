#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Compete for the leader lock. No deploy — use after deploy.sh.
# Both terminals use the same lock; one becomes leader and renews every 5s until killed.
# Usage: ./compete.sh <term1|term2> [HTTP_PORT]
# IMPORTANT: Use the SAME port in both terminals so both hit the same node and same lock.
# Example: Terminal 1: ./compete.sh term1 8092
#          Terminal 2: ./compete.sh term2 8092

set -euo pipefail

OWNER_ID="${1:?Usage: $0 <term1|term2> [HTTP_PORT]}"
HTTP_PORT="${2:-8092}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_ID="leader-election-${OWNER_ID}"
ACTOR_TYPE="$APP_ID"

# Try to acquire for up to this long (seconds)
ACQUIRE_TIMEOUT=300
# Sleep between try-acquire attempts (seconds)
ACQUIRE_POLL=5
# Renew lease every this many seconds while leader
RENEW_INTERVAL=5

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

PAYLOAD_BASE="{\"msg_type\":\"%s\",\"payload\":{\"candidate_id\":\"$OWNER_ID\"}}"

try_acquire() {
    local payload
    payload=$(printf "$PAYLOAD_BASE" "try_lead")
    local url="http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE/ask"
    local tmpbody; tmpbody=$(mktemp)
    curl -s -o "$tmpbody" --max-time 10 -X POST "$url" \
        -H "Content-Type: application/json" -d "$payload" 2>/dev/null || true
    RESPONSE=$(cat "$tmpbody" 2>/dev/null)
    rm -f "$tmpbody"
    if echo "$RESPONSE" | grep -qE '"leader"\s*:\s*true|\\"leader\\":\s*true'; then
        return 0
    fi
    return 1
}

do_renew() {
    local payload
    payload=$(printf "$PAYLOAD_BASE" "renew_lead")
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE/ask" \
        -H "Content-Type: application/json" -d "$payload" 2>/dev/null) || RESPONSE=""
    if echo "$RESPONSE" | grep -qE '"renewed"\s*:\s*true|\\"renewed\\":\s*true'; then
        return 0
    fi
    return 1
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Leader Election - Compete for lock ($OWNER_ID)         ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "  Owner: ${GREEN}${OWNER_ID}${NC}   Port: $HTTP_PORT   Acquire timeout: ${ACQUIRE_TIMEOUT}s   Renew interval: ${RENEW_INTERVAL}s"
echo ""

# Loop: try-acquire; on success → renew every 5s; else sleep 5, retry (up to 300s)
elapsed=0
while [ $elapsed -lt "$ACQUIRE_TIMEOUT" ]; do
    if try_acquire; then
        echo -e "${GREEN}✓ Acquired lock → leader ($OWNER_ID)${NC}"
        echo ""
        echo "Renewing lease every ${RENEW_INTERVAL}s (Ctrl+C to release and exit)"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        renew_count=0
        while true; do
            sleep "$RENEW_INTERVAL"
            renew_count=$((renew_count + 1))
            if do_renew; then
                echo -e "  ${GREEN}✓${NC} Renewed (${renew_count})"
            else
                echo -e "  ${YELLOW}⚠${NC} Renew failed (${renew_count})"
            fi
        done
    fi
    echo -e "  ${YELLOW}Acquiring lock...${NC} (${elapsed}s / ${ACQUIRE_TIMEOUT}s)"
    sleep "$ACQUIRE_POLL"
    elapsed=$((elapsed + ACQUIRE_POLL))
done

echo -e "${RED}✗ Timeout: could not acquire lock within ${ACQUIRE_TIMEOUT}s${NC}"
exit 1
