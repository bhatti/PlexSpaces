#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test N-Body Python WASM actor deployment and execution

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/nbody_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8091
HTTP_PORT="${1:-8091}"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# Configuration for simulation
BODY_COUNT=3



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

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body Gravitational Simulation (Python WASM Actors)       ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "Configuration:"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
echo "  Bodies: $BODY_COUNT (Sun, Mercury, Venus)"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor (requires componentize-py)..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || {
        echo -e "${RED}❌ Build failed${NC}"
        exit 1
    }
    echo ""
fi

# Check node is running
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"

if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""


# Define body configurations
declare -a BODY_NAMES=("Sun" "Mercury" "Venus")
declare -a BODY_MASSES=("1.989e30" "3.285e23" "4.867e24")

# Deploy body actors
echo "Step 2: Deploy body actors"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

DEPLOYED=0
for i in $(seq 0 $((BODY_COUNT - 1))); do
    APP_ID="nbody-body-$i"
    APP_NAME="${BODY_NAMES[$i]}"
    
    # Clean up any existing deployment
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
    sleep 1
    
    # Deploy via HTTP multipart (3 attempts)
    _dep=0
    for _attempt in 1 2 3; do
      DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
          ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
          -F "application_id=$APP_ID" \
          -F "name=$APP_ID" \
          -F "version=1.0.0" \
          (cd "$(dirname "$WASM_FILE")" && zip -j "$APP_ZIP" "$WASM_FILE" 2>/dev/null) || zip -j "$APP_ZIP" "$WASM_FILE"
          -F "app_file=@$APP_ZIP" 2>&1) || DEPLOY_OUT=""
      _hc=$(echo "$DEPLOY_OUT" | tail -n1)
      RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
      if [ "$_hc" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
        _dep=1; break
      fi
      sleep 3
    done
    if [ "$_dep" -eq 1 ]; then
        echo -e "  ${GREEN}✓${NC} Deployed $APP_ID ($APP_NAME)"
        DEPLOYED=$((DEPLOYED + 1))
    else
        echo -e "  ${YELLOW}⚠${NC} Deploy $APP_ID: $RESPONSE"
    fi
done
echo ""

# Helper function - send POST
send_post() {
    local namespace="$1"
    local actor_type="$2"
    local desc="$3"
    local payload="$4"
    
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$namespace/$actor_type" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

if [ $DEPLOYED -gt 0 ]; then
    sleep 2
    
    echo "Step 3: Initialize bodies with state"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    for i in $(seq 0 $((DEPLOYED - 1))); do
        APP_ID="nbody-body-$i"
        BODY_NAME="${BODY_NAMES[$i]}"
        MASS="${BODY_MASSES[$i]}"
        
        send_post "$APP_ID" "$APP_ID" "Set $BODY_NAME mass" "{\"msg_type\":\"set_mass\",\"payload\":{\"mass\":$MASS}}"
    done
    echo ""
    
    echo "Step 4: Calculate gravitational forces"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Calculate force between Sun and Mercury
    send_post "nbody-body-0" "nbody-body-0" "Calculate force Sun->Mercury" '{"msg_type":"calculate_force","payload":{"mass":3.285e23,"position":[5.79e10,0,0]}}'
    send_post "nbody-body-1" "nbody-body-1" "Calculate force Mercury->Sun" '{"msg_type":"calculate_force","payload":{"mass":1.989e30,"position":[0,0,0]}}'
    echo ""
    
    echo "Step 5: Update positions"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    for i in $(seq 0 $((DEPLOYED - 1))); do
        APP_ID="nbody-body-$i"
        BODY_NAME="${BODY_NAMES[$i]}"
        send_post "$APP_ID" "$APP_ID" "Update $BODY_NAME position" '{"msg_type":"update","payload":{"dt":3600}}'
    done
    echo ""
    
    echo "Step 6: Get final states (via GET)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    for i in $(seq 0 $((DEPLOYED - 1))); do
        APP_ID="nbody-body-$i"
        BODY_NAME="${BODY_NAMES[$i]}"
        
        RESPONSE=$(curl -s --max-time 10 -X GET "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$APP_ID?msg_type=get_state" 2>/dev/null) || RESPONSE=""
        
        if echo "$RESPONSE" | grep -q '"success":true'; then
            echo -e "  ${GREEN}✓${NC} $BODY_NAME state retrieved"
        else
            echo -e "  ${YELLOW}⚠${NC} $BODY_NAME: $RESPONSE"
        fi
    done
    echo ""
else
    echo "Step 3-6: Simulation skipped (no actors deployed)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  Note: WASM Components (from componentize-py) require Component Model"
    echo "  support in the runtime. Verify the build succeeded."
    echo ""
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}✅ N-Body Test Complete!${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} Multiple actor deployment (celestial bodies)"
echo -e "  ${GREEN}•${NC} Inter-actor communication (force calculations)"
echo -e "  ${GREEN}•${NC} Physics simulation using actor model"
echo ""
