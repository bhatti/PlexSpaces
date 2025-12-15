#!/bin/bash
# N-Body Simulation - Run Example

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║              N-Body Simulation Example                         ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build in release mode
echo "Building nbody example..."
cargo build --release --bin nbody

echo ""
echo "Running simulation..."
echo ""

# Run the example
cargo run --release --bin nbody
