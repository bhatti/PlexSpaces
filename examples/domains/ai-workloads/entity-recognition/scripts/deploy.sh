#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Deployment script for multi-node entity recognition example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

echo "=========================================="
echo "Entity Recognition - Multi-Node Deployment"
echo "=========================================="

# Check if Redis is running
if ! command -v redis-cli &> /dev/null; then
    echo "❌ redis-cli not found. Please install Redis."
    exit 1
fi

if ! redis-cli ping &> /dev/null; then
    echo "❌ Redis is not running. Please start Redis:"
    echo "   docker run -d -p 6379:6379 redis:7-alpine"
    exit 1
fi

echo "✓ Redis is running"

# Build
echo ""
echo "Building example..."
cd "$EXAMPLE_DIR"
cargo build --release

# Start nodes in background
echo ""
echo "Starting nodes..."
echo "Note: Press Ctrl+C to stop all nodes"

# Node 1 (CPU)
cargo run --release --bin entity-recognition-node -- \
    --node-id node-1 \
    --listen-addr 0.0.0.0:9000 \
    --backend redis \
    --redis-url redis://localhost:6379 \
    --labels workload=cpu-intensive,tier=standard &

NODE1_PID=$!

# Node 2 (GPU)
cargo run --release --bin entity-recognition-node -- \
    --node-id node-2 \
    --listen-addr 0.0.0.0:9001 \
    --backend redis \
    --redis-url redis://localhost:6379 \
    --labels workload=gpu-intensive,tier=gpu &

NODE2_PID=$!

# Node 3 (CPU)
cargo run --release --bin entity-recognition-node -- \
    --node-id node-3 \
    --listen-addr 0.0.0.0:9002 \
    --backend redis \
    --redis-url redis://localhost:6379 \
    --labels workload=cpu-intensive,tier=standard &

NODE3_PID=$!

# Wait for nodes to start
echo "Waiting for nodes to start..."
sleep 3

# Cleanup function
cleanup() {
    echo ""
    echo "Shutting down nodes..."
    kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null || true
    wait $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null || true
    echo "Nodes stopped"
}

trap cleanup EXIT INT TERM

echo "✓ Nodes started (PIDs: $NODE1_PID, $NODE2_PID, $NODE3_PID)"
echo ""
echo "Nodes are running. You can now run the application:"
echo "  cargo run --release --bin entity-recognition-app -- \\"
echo "      --node-addr http://localhost:9000 \\"
echo "      --backend redis \\"
echo "      --redis-url redis://localhost:6379 \\"
echo "      doc1.txt doc2.txt doc3.txt"
echo ""
echo "Press Ctrl+C to stop all nodes"

# Wait for user interrupt
wait

