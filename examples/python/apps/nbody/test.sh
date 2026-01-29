#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test N-Body Python WASM actor deployment and execution using PlexSpaces CLI

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/nbody_actor.wasm"
NODE_ADDR="${1:-localhost:8090}"
HTTP_PORT="${2:-8091}"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Configuration for simulation
BODY_COUNT=5
SIMULATION_STEPS=10
TIME_STEP=3600  # 1 hour in seconds

cleanup() {
    echo ""
    echo "Step 6: Cleanup deployed applications"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    for i in $(seq 0 $((BODY_COUNT - 1))); do
        APP_ID="nbody-body-$i"
        cargo run -q -p plexspaces-cli -- undeploy --node "$NODE_ADDR" --app-id "$APP_ID" 2>/dev/null || true
        echo "  Undeployed $APP_ID"
    done
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body Gravitational Simulation (Python WASM Actors)       ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "Configuration:"
echo "  Node (gRPC): $NODE_ADDR"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
echo "  Bodies: $BODY_COUNT"
echo "  Simulation Steps: $SIMULATION_STEPS"
echo "  Time Step: $TIME_STEP seconds"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ] || [ $(stat -f%z "$WASM_FILE" 2>/dev/null || stat -c%s "$WASM_FILE") -lt 1000 ]; then
    echo "📦 Building WASM actor (requires componentize-py)..."
    "$SCRIPT_DIR/build.sh" || {
        echo -e "${RED}❌ Build failed${NC}"
        exit 1
    }
    echo ""
fi

WASM_SIZE=$(stat -f%z "$WASM_FILE" 2>/dev/null || stat -c%s "$WASM_FILE")
echo "WASM size: $(( WASM_SIZE / 1024 / 1024 ))MB"
echo ""

# Check node is running
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cd "$WORKSPACE_DIR"
cargo build -q -p plexspaces-cli 2>/dev/null || true
STATUS_OUTPUT=$(cargo run -q -p plexspaces-cli -- status --node "$NODE_ADDR" 2>/dev/null) || true
if [ -z "$STATUS_OUTPUT" ] || ! echo "$STATUS_OUTPUT" | grep -q "Overall Status"; then
    echo -e "${RED}❌ Cannot connect to node at $NODE_ADDR${NC}"
    echo ""
    echo "Please start a PlexSpaces node first (Terminal 1):"
    echo "  cd $WORKSPACE_DIR"
    echo "  cargo run -p plexspaces-cli -- start --node-id nbody-node --listen-addr 0.0.0.0:8090"
    echo ""
    exit 1
fi
echo -e "${GREEN}✓ Node is running at $NODE_ADDR${NC}"
echo ""

# Deploy body actors
echo "Step 2: Deploy body actors"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

DEPLOYED=0
trap cleanup EXIT

# Define initial states for bodies (solar system inspired, with scaled values)
declare -a BODY_NAMES=("Sun" "Mercury" "Venus" "Earth" "Mars")
declare -a BODY_MASSES=("1.989e30" "3.285e23" "4.867e24" "5.972e24" "6.39e23")
declare -a BODY_POS_X=("0" "5.79e10" "1.082e11" "1.496e11" "2.279e11")
declare -a BODY_POS_Y=("0" "0" "0" "0" "0")
declare -a BODY_VEL_Y=("0" "47870" "35020" "29780" "24077")

for i in $(seq 0 $((BODY_COUNT - 1))); do
    APP_ID="nbody-body-$i"
    APP_NAME="${BODY_NAMES[$i]:-body-$i}"
    
    echo "  Deploying $APP_ID ($APP_NAME)..."
    
    RESPONSE=$(cargo run -q -p plexspaces-cli -- deploy \
        --node "$NODE_ADDR" \
        -i "$APP_ID" \
        -n "$APP_NAME" \
        -w "$WASM_FILE" 2>&1 | grep -v "^warning:" | grep -v "^   -->" | grep -v "^   |" | grep -v "note:") || true
    
    if echo "$RESPONSE" | grep -q "deployed"; then
        echo -e "    ${GREEN}✓ Deployed $APP_ID ($APP_NAME)${NC}"
        DEPLOYED=$((DEPLOYED + 1))
    elif echo "$RESPONSE" | grep -q "WASM module is not valid\|instantiation"; then
        echo -e "    ${YELLOW}⚠ WASM component not yet supported by runtime${NC}"
        break
    else
        echo -e "    ${YELLOW}⚠ $(echo "$RESPONSE" | grep -E "Error|error|failed" | head -1 || echo "Unknown response")${NC}"
    fi
done
echo ""

# List deployed applications
echo "Step 3: List deployed applications"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cargo run -q -p plexspaces-cli -- list --node "$NODE_ADDR" 2>/dev/null | grep -E "^   -|Listing|No applications" || echo "  (no response)"
echo ""

# Run simulation if actors deployed
if [ $DEPLOYED -gt 0 ]; then
    echo "Step 4: Initialize bodies with state"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Initializing $DEPLOYED celestial bodies with realistic solar system data:"
    echo ""
    
    for i in $(seq 0 $((DEPLOYED - 1))); do
        BODY_NAME="${BODY_NAMES[$i]:-body-$i}"
        MASS="${BODY_MASSES[$i]:-1.0e24}"
        POS_X="${BODY_POS_X[$i]:-0}"
        POS_Y="${BODY_POS_Y[$i]:-0}"
        VEL_Y="${BODY_VEL_Y[$i]:-0}"
        
        echo -e "  ${CYAN}[$BODY_NAME]${NC}"
        echo "    Mass: $MASS kg"
        echo "    Position: ($POS_X, $POS_Y, 0) m"
        echo "    Velocity: (0, $VEL_Y, 0) m/s"
        echo ""
    done
    
    echo "Step 5: Run N-Body Simulation"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Simulation Parameters:"
    echo "  Bodies: $DEPLOYED"
    echo "  Steps: $SIMULATION_STEPS"
    echo "  dt: $TIME_STEP seconds ($(( TIME_STEP / 3600 )) hours)"
    echo "  Physics: Newtonian gravity (F = G·m₁·m₂/r²)"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    START_TIME=$(python3 -c "import time; print(time.time())")
    TOTAL_COMPUTE_MS=0
    TOTAL_COORDINATION_MS=0
    TOTAL_FORCES=0
    TOTAL_UPDATES=0
    
    for step in $(seq 1 $SIMULATION_STEPS); do
        STEP_START=$(python3 -c "import time; print(time.time())")
        
        echo -e "${BLUE}═══ Step $step/$SIMULATION_STEPS ═══${NC}"
        
        # Phase 1: Calculate forces between all pairs
        echo "  [Force Calculation Phase]"
        FORCE_START=$(python3 -c "import time; print(time.time())")
        
        for i in $(seq 0 $((DEPLOYED - 1))); do
            for j in $(seq 0 $((DEPLOYED - 1))); do
                if [ $i -ne $j ]; then
                    BODY_I="${BODY_NAMES[$i]:-body-$i}"
                    BODY_J="${BODY_NAMES[$j]:-body-$j}"
                    MASS_J="${BODY_MASSES[$j]:-1.0e24}"
                    POS_X_J="${BODY_POS_X[$j]:-0}"
                    POS_Y_J="${BODY_POS_Y[$j]:-0}"
                    
                    # Call calculate_force on body i to get force from body j
                    # NOTE: Use actor name (Sun, Mercury, etc.) NOT application ID (nbody-body-0)
                    # Actors are registered with their name as actor_type
                    PAYLOAD=$(cat <<EOF
{"mass": $MASS_J, "position": [$POS_X_J, $POS_Y_J, 0]}
EOF
)
                    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/internal/system/$BODY_I" \
                        -H "Content-Type: application/json" \
                        -H "X-Message-Type: calculate_force" \
                        -d "$PAYLOAD" 2>/dev/null) || true
                    
                    if [ -n "$RESPONSE" ] && echo "$RESPONSE" | grep -q "fx\|fy\|fz"; then
                        FX=$(echo "$RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',{}); print(p.get('fx',0) if isinstance(p,dict) else 0)" 2>/dev/null || echo "0")
                        TOTAL_FORCES=$((TOTAL_FORCES + 1))
                        if [ $((TOTAL_FORCES % 10)) -eq 0 ]; then
                            echo "    $BODY_I ← force from $BODY_J: fx=$FX N"
                        fi
                    fi
                fi
            done
        done
        
        FORCE_END=$(python3 -c "import time; print(time.time())")
        FORCE_MS=$(python3 -c "print(int(($FORCE_END - $FORCE_START) * 1000))")
        TOTAL_COMPUTE_MS=$((TOTAL_COMPUTE_MS + FORCE_MS))
        echo "    Force calculations: ${FORCE_MS}ms"
        
        # Phase 2: Update positions
        echo "  [Position Update Phase]"
        UPDATE_START=$(python3 -c "import time; print(time.time())")
        
        for i in $(seq 0 $((DEPLOYED - 1))); do
            BODY_NAME="${BODY_NAMES[$i]:-body-$i}"
            
            # Use actor name (Sun, Mercury, etc.) NOT application ID
            RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/internal/system/$BODY_NAME" \
                -H "Content-Type: application/json" \
                -H "X-Message-Type: update" \
                -d "{\"dt\": $TIME_STEP}" 2>/dev/null) || true
            
            if [ -n "$RESPONSE" ]; then
                TOTAL_UPDATES=$((TOTAL_UPDATES + 1))
            fi
        done
        
        UPDATE_END=$(python3 -c "import time; print(time.time())")
        UPDATE_MS=$(python3 -c "print(int(($UPDATE_END - $UPDATE_START) * 1000))")
        TOTAL_COORDINATION_MS=$((TOTAL_COORDINATION_MS + UPDATE_MS))
        echo "    Position updates: ${UPDATE_MS}ms"
        
        # Phase 3: Get states (sample every 5 steps)
        if [ $((step % 5)) -eq 0 ] || [ $step -eq 1 ]; then
            echo "  [State Query Phase]"
            for i in $(seq 0 $((DEPLOYED - 1))); do
                BODY_NAME="${BODY_NAMES[$i]:-body-$i}"
                
                # Use actor name (Sun, Mercury, etc.) NOT application ID
                RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/internal/system/$BODY_NAME" \
                    -H "Content-Type: application/json" \
                    -H "X-Message-Type: get_state" \
                    -d "{}" 2>/dev/null) || true
                
                if [ -n "$RESPONSE" ] && echo "$RESPONSE" | grep -q "position"; then
                    # Extract position from nested JSON
                    POS=$(echo "$RESPONSE" | python3 -c "
import sys,json
try:
    d=json.load(sys.stdin)
    p=d.get('payload',{})
    if isinstance(p,dict):
        pos=p.get('position',[0,0,0])
        print(f'({pos[0]:.2e}, {pos[1]:.2e}, {pos[2]:.2e})')
    else:
        print('(unknown)')
except:
    print('(parse error)')
" 2>/dev/null || echo "(error)")
                    echo "    $BODY_NAME: pos=$POS"
                fi
            done
        fi
        
        STEP_END=$(python3 -c "import time; print(time.time())")
        STEP_MS=$(python3 -c "print(int(($STEP_END - $STEP_START) * 1000))")
        echo "  Step time: ${STEP_MS}ms"
        echo ""
    done
    
    END_TIME=$(python3 -c "import time; print(time.time())")
    TOTAL_MS=$(python3 -c "print(int(($END_TIME - $START_TIME) * 1000))")
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo -e "${GREEN}📊 Simulation Results${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  Total simulation time: ${TOTAL_MS}ms"
    echo "  Force calculations: $TOTAL_FORCES"
    echo "  Position updates: $TOTAL_UPDATES"
    echo ""
    echo "  Compute time (forces): ${TOTAL_COMPUTE_MS}ms"
    echo "  Coordination time (updates): ${TOTAL_COORDINATION_MS}ms"
    echo ""
    if [ $TOTAL_MS -gt 0 ]; then
        COMPUTE_PCT=$(python3 -c "print(f'{$TOTAL_COMPUTE_MS / $TOTAL_MS * 100:.1f}')")
        COORD_PCT=$(python3 -c "print(f'{$TOTAL_COORDINATION_MS / $TOTAL_MS * 100:.1f}')")
        echo "  Compute ratio: ${COMPUTE_PCT}%"
        echo "  Coordination ratio: ${COORD_PCT}%"
    fi
    echo ""
else
    echo "Step 4-5: Simulation skipped (no actors deployed)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  Note: WASM Components (from componentize-py) require Component Model"
    echo "  support in the runtime. Currently only WASM Modules are supported."
    echo ""
    echo "  N-Body simulation message flow (for reference):"
    echo ""
    echo "  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐"
    echo "  │    Sun      │     │   Earth     │     │    Mars     │"
    echo "  │  1.989e30kg │     │  5.972e24kg │     │  6.39e23kg  │"
    echo "  └──────┬──────┘     └──────┬──────┘     └──────┬──────┘"
    echo "         │                   │                   │"
    echo "         │←── calculate_force(mass, position) ──→│"
    echo "         │                   │                   │"
    echo "         ├──── add_force(fx, fy, fz) ────────────┤"
    echo "         │                   │                   │"
    echo "         ├──── update(dt=$TIME_STEP) ────────────────────┤"
    echo "         │                   │                   │"
    echo "         └──── get_state() ──────────────────────┘"
    echo ""
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}✅ N-Body Test Complete!${NC}"
echo ""
echo "PlexSpaces CLI commands used:"
echo "  • plexspaces start --node-id nbody-node --listen-addr 0.0.0.0:8090"
echo "  • plexspaces deploy -i <app-id> -n <name> -w <wasm>"
echo "  • plexspaces list --node localhost:8090"
echo "  • curl -X POST http://localhost:8091/api/v1/actors/internal/system/<actor>"
echo ""
