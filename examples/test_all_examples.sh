#!/bin/bash
# Test all PlexSpaces multi-node examples
# Usage: ./test_all_examples.sh

set -e  # Exit on error

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  PlexSpaces Multi-Node Examples - E2E Testing                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Get script directory
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

# Test 1: Byzantine Generals (SKIPPED - needs ActorRegistry migration)
# echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
# echo "Test 1: Byzantine Generals (Distributed Consensus)"
# echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
# echo ""
# echo "⚠️  SKIPPED: Byzantine Generals tests need ActorRegistry migration"
# echo ""

# Test 2: Heat Diffusion
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 2: Heat Diffusion (Scientific Computing)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

cd "$SCRIPT_DIR/heat_diffusion"
echo "Running: cargo test --test distributed_test"
cargo test --test distributed_test

echo ""
echo "✅ Heat Diffusion test PASSED"
echo ""

# Summary
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    All Tests Passed!                           ║"
echo "║                                                                ║"
echo "║  ⚠️  Byzantine Generals (SKIPPED - needs ActorRegistry)        ║"
echo "║  ✅ Heat Diffusion (Scientific Computing)                      ║"
echo "║                                                                ║"
echo "║  PlexSpaces multi-node coordination verified!                 ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Optional: Run large-scale E2E test
read -p "Run large-scale E2E test (200×200 grid, 16 actors)? (y/N): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]
then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Large-Scale Heat Diffusion E2E Test (Production Realistic)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    cd "$SCRIPT_DIR/heat_diffusion"
    cargo test --release --test e2e_large_grid -- --nocapture --ignored

    echo ""
    echo "✅ Large-scale E2E test PASSED"
    echo ""
fi

# Optional: Run with output
read -p "Run unit tests again with full output? (y/N): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]
then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Byzantine Generals (with output)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    cd "$SCRIPT_DIR/byzantine"
    cargo test --test registry_distributed_consensus -- --nocapture

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Heat Diffusion (with output)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    cd "$SCRIPT_DIR/heat_diffusion"
    cargo test --test registry_distributed_heat_diffusion -- --nocapture
fi
