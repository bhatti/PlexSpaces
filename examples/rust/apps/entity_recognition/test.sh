#!/bin/bash
# Entity Recognition Example - Test Script
#
# Demonstrates FSM, GenServer, and GenEvent actor patterns
#
# Usage: ./test.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "╔════════════════════════════════════════════════════════════════════╗"
echo "║  Entity Recognition Example - Test                                 ║"
echo "║  Showcases: FSM, GenServer, GenEvent patterns                      ║"
echo "╚════════════════════════════════════════════════════════════════════╝"
echo

# Build
echo "Building..."
cargo build --release 2>&1 | tail -5

# Run
echo
echo "Running entity_recognition example..."
echo "────────────────────────────────────────────────────────────────────"
cargo run --release 2>&1

echo
echo "════════════════════════════════════════════════════════════════════"
echo "✅ Test complete!"
echo "════════════════════════════════════════════════════════════════════"
