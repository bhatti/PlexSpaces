#!/bin/bash
# Quick test - assumes everything is already built

set -e

cd "$(dirname "$0")/.."

echo "Quick Test: Starting node and deploying WASM application..."
echo ""

# Check if WASM file exists
if [ ! -f "wasm-modules/nbody-application.wasm" ]; then
    echo "❌ WASM file not found. Run ./scripts/test.sh first to build everything."
    exit 1
fi

# Start node
export PLEXSPACES_NODE_ID="nbody-test-node"
export PLEXSPACES_LISTEN_ADDR="0.0.0.0:9001"

cargo run --release --bin node-starter > /tmp/nbody-node.log 2>&1 &
NODE_PID=$!

echo "Node starting (PID: $NODE_PID)..."

# Wait for node
for i in {1..20}; do
    if grep -q "Node started successfully" /tmp/nbody-node.log 2>/dev/null; then
        break
    fi
    sleep 1
done

sleep 2

# Deploy
echo "Deploying application..."
cargo run --release --bin nbody-wasm -- deploy \
    --wasm wasm-modules/nbody-application.wasm \
    --node http://localhost:9001 \
    --name nbody-simulation \
    --version 0.1.0

# Cleanup
kill $NODE_PID 2>/dev/null || true
wait $NODE_PID 2>/dev/null || true

echo ""
echo "✅ Quick test complete!"

