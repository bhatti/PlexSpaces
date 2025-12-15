#!/bin/bash
# Deploy WASM application to node

set -e

cd "$(dirname "$0")/.."

NODE="${NODE:-http://localhost:9001}"
WASM="${WASM:-wasm-modules/nbody-application.wasm}"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     N-Body WASM - Deploy Script                                ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Check if WASM file exists
if [ ! -f "$WASM" ]; then
    echo "❌ WASM file not found: $WASM"
    echo "   Run ./scripts/build.sh first to build the WASM module"
    exit 1
fi

echo "Deploying WASM application:"
echo "  Node: $NODE"
echo "  WASM: $WASM"
echo ""

# Deploy using cargo run
cargo run --release --bin nbody-wasm -- deploy \
    --wasm "$WASM" \
    --node "$NODE"

echo ""
echo "✅ Deployment complete!"

