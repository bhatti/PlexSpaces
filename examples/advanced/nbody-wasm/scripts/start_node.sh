#!/bin/bash
# Start PlexSpaces node for testing

set -e

cd "$(dirname "$0")/.."

NODE_ID="${PLEXSPACES_NODE_ID:-nbody-test-node}"
LISTEN_ADDR="${PLEXSPACES_LISTEN_ADDR:-0.0.0.0:8000}"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Starting PlexSpaces Node                                  ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "Node ID: $NODE_ID"
echo "Listen Address: $LISTEN_ADDR"
echo ""
echo "Press Ctrl+C to stop the node"
echo ""

# Build if needed
if [ ! -f "target/release/node-starter" ]; then
    echo "Building node-starter..."
    cargo build --release --bin node-starter
fi

# Start node
export PLEXSPACES_NODE_ID="$NODE_ID"
export PLEXSPACES_LISTEN_ADDR="$LISTEN_ADDR"
exec cargo run --release --bin node-starter

