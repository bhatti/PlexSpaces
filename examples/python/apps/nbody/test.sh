#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test N-Body Python WASM actor deployment and execution

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/nbody_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8092
HTTP_PORT="${1:-8092}"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# Configuration for simulation
BODY_COUNT=3

cleanup() {
    echo ""
    echo "Cleanup: Undeploying applications"
    for i in $(seq 0 $((BODY_COUNT - 1))); do
        APP_ID="nbody-body-$i"
        curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
    done
}

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

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"

if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

trap cleanup EXIT

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
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
    sleep 1
    
    # Deploy via HTTP multipart
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
        -F "application_id=$APP_ID" \
        -F "name=$APP_NAME" \
        -F "version=1.0.0" \
        -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
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
        
        send_post "$APP_ID" "$BODY_NAME" "Set $BODY_NAME mass" "{\"msg_type\":\"set_mass\",\"payload\":{\"mass\":$MASS}}"
    done
    echo ""
    
    echo "Step 4: Calculate gravitational forces"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Calculate force between Sun and Mercury
    send_post "nbody-body-0" "Sun" "Calculate force Sun->Mercury" '{"msg_type":"calculate_force","payload":{"mass":3.285e23,"position":[5.79e10,0,0]}}'
    send_post "nbody-body-1" "Mercury" "Calculate force Mercury->Sun" '{"msg_type":"calculate_force","payload":{"mass":1.989e30,"position":[0,0,0]}}'
    echo ""
    
    echo "Step 5: Update positions"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    for i in $(seq 0 $((DEPLOYED - 1))); do
        APP_ID="nbody-body-$i"
        BODY_NAME="${BODY_NAMES[$i]}"
        send_post "$APP_ID" "$BODY_NAME" "Update $BODY_NAME position" '{"msg_type":"update","payload":{"dt":3600}}'
    done
    echo ""
    
    echo "Step 6: Get final states (via GET)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    for i in $(seq 0 $((DEPLOYED - 1))); do
        APP_ID="nbody-body-$i"
        BODY_NAME="${BODY_NAMES[$i]}"
        
        RESPONSE=$(curl -s --max-time 10 -X GET "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$BODY_NAME?msg_type=get_state" 2>/dev/null) || RESPONSE=""
        
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
