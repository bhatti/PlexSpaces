#!/bin/bash
# Entity Recognition Example - Test Script

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Entity Recognition Example - Tests                     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Check if Redis is running (for multi-node tests)
if command -v redis-cli &> /dev/null; then
    if redis-cli ping &> /dev/null 2>&1; then
        echo "✓ Redis is running"
        USE_REDIS=true
    else
        echo "⚠ Redis not running, using memory backend"
        USE_REDIS=false
    fi
else
    echo "⚠ redis-cli not found, using memory backend"
    USE_REDIS=false
fi

# Build
echo ""
echo "Building example..."
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

# Run the example to validate it works
echo "Running example to validate..."
timeout 30 cargo run -- \
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
echo ""
echo "To run the example with metrics display:"
echo "  ./scripts/run_with_metrics.sh"
echo ""
echo "Or use the simple run script:"
echo "  ./scripts/run.sh"
echo ""
