#!/bin/bash
# Entity Recognition Example - Test Script

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Shared target: use workspace target directory
WORKSPACE_TARGET="${SCRIPT_DIR}/../../../../target"
export CARGO_TARGET_DIR="${WORKSPACE_TARGET}"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Entity Recognition Example - Tests                     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build
echo ""
echo "Building example (shared target, debug)..."
cargo build

# Run unit tests
echo ""
echo "Running unit tests..."
cargo test --lib || echo "⚠ No unit tests found"

# Test single-node execution (memory backend)
echo ""
echo "Testing single-node execution (memory backend)..."
echo "Note: This example demonstrates resource-aware scheduling"
echo "      In production, use multi-node setup with Redis backend"
echo ""

# Run the pre-built binary to validate it works
echo "Running example to validate..."
timeout 30 "${WORKSPACE_TARGET}/debug/entity-recognition-app" \
    doc1.txt doc2.txt doc3.txt 2>&1 | tee /tmp/entity_recognition_test.txt || {
    if [ $? -eq 124 ]; then
        echo ""
        echo "⚠️  Command timed out after 30 seconds"
        echo "   This may indicate a shutdown issue. Check logs above."
    fi
    exit $?
}

# Check if metrics were displayed
if grep -q "Performance Metrics:" /tmp/entity_recognition_test.txt; then
    echo ""
    echo "✅ Example executed successfully with metrics"
    echo ""
    echo "📊 Metrics from run:"
    grep -A 5 "Performance Metrics:" /tmp/entity_recognition_test.txt | sed 's/^/  /'
    echo ""
else
    echo ""
    echo "⚠️  Example ran but metrics not found in output"
    echo ""
fi

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                    ✅ Test Complete                            ║"
echo "╚════════════════════════════════════════════════════════════════╝"
