#!/bin/bash
# Entity Recognition Example - Run with Metrics Display

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Entity Recognition Example                            ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build the default application binary in debug mode.
echo "Building entity recognition example..."
cargo build

echo ""
echo "Running entity recognition application..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Run the example with sample documents and capture output
# Use timeout to prevent stalling
timeout 30 cargo run -- \
    doc1.txt doc2.txt doc3.txt 2>&1 | tee /tmp/entity_recognition_output.txt || {
    if [ $? -eq 124 ]; then
        echo ""
        echo "⚠️  Command timed out after 30 seconds"
        echo "   This may indicate a shutdown issue. Check logs above."
    fi
    exit $?
}

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Extract and display metrics if available
if grep -q "Performance Metrics:" /tmp/entity_recognition_output.txt; then
    echo ""
    echo "📊 Key Metrics Summary:"
    grep -A 5 "Performance Metrics:" /tmp/entity_recognition_output.txt | sed 's/^/  /'
    echo ""
fi
