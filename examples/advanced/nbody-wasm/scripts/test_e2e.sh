#!/bin/bash
# End-to-end test for nbody-wasm example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
NODE_PID=""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Cleanup function
cleanup() {
    if [ ! -z "$NODE_PID" ]; then
        echo ""
        echo -e "${YELLOW}Stopping node (PID: $NODE_PID)...${NC}"
        kill $NODE_PID 2>/dev/null || true
        wait $NODE_PID 2>/dev/null || true
    fi
}

trap cleanup EXIT

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body WASM - End-to-End Test                             ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Check prerequisites
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 1: Checking prerequisites"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

MISSING_DEPS=0

if ! command -v npm &> /dev/null; then
    echo -e "${RED}❌ npm not found${NC}"
    echo "   Install Node.js and npm: https://nodejs.org/"
    MISSING_DEPS=1
else
    echo -e "${GREEN}✓ npm found${NC}"
fi

if ! command -v javy &> /dev/null; then
    echo -e "${YELLOW}⚠️  javy not found${NC}"
    echo ""
    echo "   To install Javy, run:"
    echo "   cargo install --git https://github.com/bytecodealliance/javy javy-cli"
    echo ""
    echo "   Or download from: https://github.com/bytecodealliance/javy/releases"
    echo ""
    MISSING_DEPS=1
fi

if command -v javy &> /dev/null; then
    echo -e "${GREEN}✓ javy found${NC}"
else
    echo -e "${RED}❌ javy not available${NC}"
    MISSING_DEPS=1
fi

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ cargo not found${NC}"
    MISSING_DEPS=1
else
    echo -e "${GREEN}✓ cargo found${NC}"
fi

if [ $MISSING_DEPS -eq 1 ]; then
    echo ""
    echo -e "${RED}❌ Missing prerequisites. Please install them and try again.${NC}"
    exit 1
fi

echo ""

# Step 2: Install TypeScript dependencies
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 2: Installing TypeScript dependencies"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cd "$EXAMPLE_DIR/ts-actors"
if [ ! -d "node_modules" ]; then
    echo "Installing npm dependencies..."
    npm install || {
        echo -e "${RED}❌ npm install failed${NC}"
        exit 1
    }
fi
echo -e "${GREEN}✓ Dependencies installed${NC}"
echo ""

# Step 3: Build Rust binaries
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 3: Building Rust binaries"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cd "$EXAMPLE_DIR"
cargo build --release --bin nbody-wasm --bin node-starter || {
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
}
echo -e "${GREEN}✓ Build successful${NC}"
echo ""

# Step 4: Build TypeScript to WASM
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 4: Building TypeScript to WASM"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cargo run --release --bin nbody-wasm -- build || {
    echo -e "${RED}❌ WASM build failed${NC}"
    exit 1
}

if [ ! -f "wasm-modules/nbody-application.wasm" ]; then
    echo -e "${RED}❌ WASM file not found: wasm-modules/nbody-application.wasm${NC}"
    exit 1
fi

WASM_SIZE=$(stat -f%z wasm-modules/nbody-application.wasm 2>/dev/null || stat -c%s wasm-modules/nbody-application.wasm 2>/dev/null)
echo -e "${GREEN}✓ WASM module created: wasm-modules/nbody-application.wasm (${WASM_SIZE} bytes)${NC}"
echo ""

# Step 5: Start PlexSpaces node
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 5: Starting PlexSpaces node"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

export PLEXSPACES_NODE_ID="nbody-test-node"
export PLEXSPACES_LISTEN_ADDR="0.0.0.0:9001"

# Start node in background
cargo run --release --bin node-starter > /tmp/nbody-node.log 2>&1 &
NODE_PID=$!

echo "Node starting (PID: $NODE_PID)..."
echo "Logs: /tmp/nbody-node.log"

# Wait for node to be ready
echo "Waiting for node to be ready..."
for i in {1..30}; do
    if curl -s http://localhost:9001 > /dev/null 2>&1 || grep -q "Node started successfully" /tmp/nbody-node.log 2>/dev/null; then
        echo -e "${GREEN}✓ Node is ready${NC}"
        break
    fi
    if [ $i -eq 30 ]; then
        echo -e "${RED}❌ Node failed to start within 30 seconds${NC}"
        echo "Node logs:"
        cat /tmp/nbody-node.log
        exit 1
    fi
    sleep 1
done
echo ""

# Step 6: Deploy WASM application
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 6: Deploying WASM application"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cargo run --release --bin nbody-wasm -- deploy \
    --wasm wasm-modules/nbody-application.wasm \
    --node http://localhost:9001 \
    --name nbody-simulation \
    --version 0.1.0 || {
    echo -e "${RED}❌ Deployment failed${NC}"
    echo "Node logs:"
    tail -20 /tmp/nbody-node.log
    exit 1
}

echo -e "${GREEN}✓ Application deployed successfully${NC}"
echo ""

# Step 7: Verify deployment
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 7: Test complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

echo -e "${GREEN}✅ All tests passed!${NC}"
echo ""
echo "Summary:"
echo "  ✓ Prerequisites checked"
echo "  ✓ TypeScript dependencies installed"
echo "  ✓ Rust binaries built"
echo "  ✓ WASM module built"
echo "  ✓ Node started"
echo "  ✓ Application deployed"
echo ""
echo "Node is still running (PID: $NODE_PID)"
echo "Press Ctrl+C to stop the node and exit"

# Wait for user interrupt
wait $NODE_PID

