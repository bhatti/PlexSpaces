#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Compete for the leader lock indefinitely. No deploy — use after deploy.sh.
# Tries to acquire the lock, then renews every 5s until Ctrl+C.
# Usage: ./compete.sh <actor1|actor2> [HTTP_PORT]
# Example: Terminal 1: ./compete.sh actor1 8091
#          Terminal 2: ./compete.sh actor2 8091

set -euo pipefail

ACTOR_NAME="${1:?Usage: $0 <actor1|actor2> [HTTP_PORT]}"
HTTP_PORT="${2:-8091}"

APP_ID="leader-election"
ACTOR_TYPE="leader_election"

ACQUIRE_TIMEOUT=300
ACQUIRE_POLL=5
RENEW_INTERVAL=5

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

ask() {
    curl -s --max-time 10 -X POST \
        "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_NAME}:${ACTOR_TYPE}/ask?timeout=10" \
        -H "Content-Type: application/json" \
        -d "$1" 2>/dev/null || echo '{}'
}

try_acquire() {
    local r; r=$(ask "{\"op\":\"try_lead\",\"candidate_id\":\"$ACTOR_NAME\"}")
    echo "$r" | grep -qE '"leader"[[:space:]]*:[[:space:]]*true'
}

do_renew() {
    local r; r=$(ask "{\"op\":\"renew_lead\",\"lease_duration_secs\":30,\"candidate_id\":\"$ACTOR_NAME\"}")
    echo "$r" | grep -qE '"renewed"[[:space:]]*:[[:space:]]*true'
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Leader Election - Compete ($ACTOR_NAME)                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo -e "  Actor: ${GREEN}${APP_ID}/${ACTOR_NAME}:${ACTOR_TYPE}${NC}   Port: $HTTP_PORT"
echo ""

elapsed=0
while [ $elapsed -lt "$ACQUIRE_TIMEOUT" ]; do
    if try_acquire; then
        echo -e "${GREEN}Acquired lock → leader ($ACTOR_NAME)${NC}"
        echo "Renewing every ${RENEW_INTERVAL}s (Ctrl+C to stop)"
        renew_count=0
        while true; do
            sleep "$RENEW_INTERVAL"
            renew_count=$((renew_count + 1))
            if do_renew; then
                echo -e "  ${GREEN}Renewed (${renew_count})${NC}"
            else
                echo -e "  ${YELLOW}Renew failed (${renew_count})${NC}"
            fi
        done
    fi
    echo -e "  ${YELLOW}Acquiring... (${elapsed}s / ${ACQUIRE_TIMEOUT}s)${NC}"
    sleep "$ACQUIRE_POLL"
    elapsed=$((elapsed + ACQUIRE_POLL))
done

echo -e "${RED}Timeout: could not acquire lock within ${ACQUIRE_TIMEOUT}s${NC}"
exit 1
