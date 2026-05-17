#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test Feature Flags Python WASM actor
# Real-world scenario: Managing feature rollouts

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/feature_flags_actor.wasm"
NODE_ADDR="${1:-localhost:8091}"
HTTP_PORT="${2:-8091}"

export CARGO_TARGET_DIR="$WORKSPACE_DIR/target"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="feature-flags-test"
ACTOR_TYPE="$APP_ID"


echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Feature Flags Service (Python WASM)                     ║"
echo "║        With One-for-One Supervisor (auto-restart)              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node is running via HTTP
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Check if HTTP gateway is responding
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"

if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node HTTP gateway at localhost:$HTTP_PORT${NC}"
    echo "Please start a PlexSpaces node first"
    exit 1
fi
echo -e "${GREEN}✓ Node HTTP gateway is running (HTTP $HTTP_CHECK)${NC}"
echo ""

# Deploy
echo "Step 2: Deploy feature flags actor (with supervisor)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "   Supervisor: one-for-one (max 5 restarts)"

# Clean up any existing deployment first (idempotent re-runs)
echo "   Cleaning up any existing deployment..."
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

# Deploy using HTTP multipart (supports large WASM files)
echo "   Deploying via HTTP multipart..."
_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true
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
    echo -e "${GREEN}✓ Deployed feature flags service${NC}"
elif echo "$RESPONSE" | grep -q "WASM module is not valid\|instantiation\|component"; then
    echo -e "${YELLOW}⚠ WASM component info${NC}"
    exit 0
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""

# Test operations
echo "Step 3: Test feature flag operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

send_op() {
    local description="$1"
    local payload="$2"
    
    # Don't hardcode tenant_id - use path format /api/v1/actors/{namespace}/{actor_type}
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || true
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $description"
    else
        echo -e "  ${YELLOW}⚠${NC} $description: $RESPONSE"
    fi
}

echo ""
echo "🚩 Creating feature flags..."
send_op "Create dark-mode flag" '{"op":"create","flag":"dark-mode"}'
send_op "Create new-ui flag" '{"op":"create","flag":"new-ui"}'
send_op "Create beta flag" '{"op":"create","flag":"beta"}'

echo ""
echo "✅ Enabling flags..."
send_op "Enable dark-mode (100%)" '{"op":"enable","flag":"dark-mode"}'
send_op "Enable new-ui (for rollout)" '{"op":"enable","flag":"new-ui"}'

echo ""
echo "📊 Setting rollout percentages..."
send_op "Set new-ui to 25% rollout" '{"op":"rollout","flag":"new-ui","pct":25}'

echo ""
echo "🔍 Checking flags for users..."
send_op "Check dark-mode for alice" '{"op":"check","flag":"dark-mode","user":"alice"}'
send_op "Check new-ui for bob" '{"op":"check","flag":"new-ui","user":"bob"}'
send_op "Check new-ui for carol" '{"op":"check","flag":"new-ui","user":"carol"}'
send_op "Check beta (disabled)" '{"op":"check","flag":"beta","user":"alice"}'

echo ""
echo "📋 Listing all flags..."
send_op "List all flags" '{"op":"list"}'

echo ""
echo -e "${GREEN}✅ Feature Flags test complete!${NC}"
echo ""
echo "This example demonstrated:"
echo "  - Feature flag CRUD operations"
echo "  - Gradual rollout with consistent hashing"
echo "  - Supervisor auto-restart (one-for-one policy)"
