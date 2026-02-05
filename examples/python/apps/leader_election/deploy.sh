#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Deploy leader election app for BOTH competitors (term1 and term2).
# Run once before using compete.sh from two terminals.
# Usage: ./deploy.sh [HTTP_PORT]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/leader_election_actor.wasm"
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

ACTOR_NAME="LeaderElection"

# Build WASM if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Deploy for both term1 and term2 (two apps = two actors contending for same lock)
for OWNER in term1 term2; do
    APP_ID="leader-election-${OWNER}"
    echo "Deploying $APP_ID..."
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
        -F "application_id=$APP_ID" \
        -F "name=$ACTOR_NAME" \
        -F "version=1.0.0" \
        -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true
    if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
        echo -e "${GREEN}✓ Deployed $APP_ID${NC}"
    else
        echo -e "${RED}✗ Deploy $APP_ID failed: $RESPONSE${NC}"
        exit 1
    fi
done

echo ""
echo -e "${GREEN}✓ Done. Run from two terminals: ./compete.sh term1 $HTTP_PORT  and  ./compete.sh term2 $HTTP_PORT${NC}"
echo ""
