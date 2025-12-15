#!/bin/bash
# Simple end-to-end test script for nbody-wasm example

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body WASM - Simple End-to-End Test                      ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Check prerequisites
echo "Checking prerequisites..."
MISSING=0

if ! command -v npm &> /dev/null; then
    echo "❌ npm not found - install Node.js from https://nodejs.org/"
    MISSING=1
else
    echo "✓ npm found"
fi

# Check for javy in PATH or common locations
JAVY_PATH=""
if command -v javy &> /dev/null; then
    JAVY_PATH="javy"
elif [ -f "$HOME/.local/bin/javy" ]; then
    JAVY_PATH="$HOME/.local/bin/javy"
    export PATH="$PATH:$HOME/.local/bin"
fi

if [ -z "$JAVY_PATH" ]; then
    echo "⚠️  javy not found"
    echo "   Will use Rust WASM fallback for testing"
    echo "   To use TypeScript actors, install javy:"
    echo "     ./scripts/install_javy.sh"
    echo "     OR: Download from https://github.com/bytecodealliance/javy/releases"
    USE_RUST_WASM=1
else
    echo "✓ javy found: $JAVY_PATH"
    USE_RUST_WASM=0
fi

if ! command -v cargo &> /dev/null; then
    echo "❌ cargo not found - install Rust from https://rustup.rs/"
    MISSING=1
else
    echo "✓ cargo found"
fi

if [ $MISSING -eq 1 ]; then
    echo ""
    echo "❌ Missing prerequisites. Please install them and try again."
    exit 1
fi

echo "✓ All prerequisites met"
echo ""

# Step 1: Install TypeScript dependencies
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 1: Installing TypeScript dependencies"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd ts-actors
if [ ! -d "node_modules" ]; then
    npm install
fi
echo "✓ Dependencies installed"
echo ""

# Step 2: Build Rust binaries
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 2: Building Rust binaries"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd ..
cargo build --release --bin nbody-wasm --bin node-starter
echo "✓ Rust binaries built"
echo ""

# Step 3: Build WASM module
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 3: Building WASM module"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ "$USE_RUST_WASM" -eq 1 ]; then
    echo "Using Rust WASM fallback (javy not available)..."
    ./scripts/build_rust_wasm.sh
else
    echo "Building TypeScript to WASM..."
    cargo run --release --bin nbody-wasm -- build
fi

if [ ! -f "wasm-modules/nbody-application.wasm" ]; then
    echo "❌ WASM file not found"
    exit 1
fi
WASM_SIZE=$(stat -f%z wasm-modules/nbody-application.wasm 2>/dev/null || stat -c%s wasm-modules/nbody-application.wasm 2>/dev/null)
echo "✓ WASM module built: wasm-modules/nbody-application.wasm ($WASM_SIZE bytes)"
echo ""

# Step 4: Start node in background
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 4: Starting PlexSpaces node"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
export PLEXSPACES_NODE_ID="nbody-test-node"
export PLEXSPACES_LISTEN_ADDR="0.0.0.0:9001"

# Start node in background
cargo run --release --bin node-starter > /tmp/nbody-node.log 2>&1 &
NODE_PID=$!

echo "Node starting (PID: $NODE_PID)..."
echo "Logs: /tmp/nbody-node.log"

# Wait for node to be ready (check logs for success message)
for i in {1..30}; do
    if grep -q "Node started successfully" /tmp/nbody-node.log 2>/dev/null; then
        echo "✓ Node is ready"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "❌ Node failed to start within 30 seconds"
        echo "Node logs:"
        cat /tmp/nbody-node.log
        kill $NODE_PID 2>/dev/null || true
        exit 1
    fi
    sleep 1
done

# Give node a moment to fully initialize
sleep 2
echo ""

# Step 5: Deploy WASM application
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 5: Deploying WASM application"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cargo run --release --bin nbody-wasm -- deploy \
    --wasm wasm-modules/nbody-application.wasm \
    --node http://localhost:9001 \
    --name nbody-simulation \
    --version 0.1.0

if [ $? -eq 0 ]; then
    echo "✓ Application deployed successfully"
else
    echo "❌ Deployment failed"
    echo "Node logs:"
    tail -20 /tmp/nbody-node.log
    kill $NODE_PID 2>/dev/null || true
    exit 1
fi
echo ""

# Step 6: Cleanup
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 6: Cleaning up"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Stopping node (PID: $NODE_PID)..."
if ps -p $NODE_PID > /dev/null 2>&1; then
    # Try graceful shutdown first
    kill $NODE_PID 2>/dev/null || true
    # Wait up to 3 seconds
    for i in {1..6}; do
        if ! ps -p $NODE_PID > /dev/null 2>&1; then
            break
        fi
        sleep 0.5
    done
    # Force kill if still running
    if ps -p $NODE_PID > /dev/null 2>&1; then
        kill -9 $NODE_PID 2>/dev/null || true
        sleep 0.5
    fi
fi
echo "✓ Node stopped"
echo ""

# Success
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    ✅ Test Complete                            ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "Summary:"
echo "  ✓ Prerequisites checked"
echo "  ✓ TypeScript dependencies installed"
echo "  ✓ Rust binaries built"
echo "  ✓ WASM module built"
echo "  ✓ Node started"
echo "  ✓ Application deployed"
echo ""
echo "All tests passed! 🎉"

