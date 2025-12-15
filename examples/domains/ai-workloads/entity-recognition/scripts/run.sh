#!/bin/bash
# Entity Recognition Example - Run Script

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Entity Recognition Example                            ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build in release mode
echo "Building entity recognition example..."
cargo build --release --bin entity-recognition-app

echo ""
echo "Running entity recognition application..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Run the example with sample documents
# Default documents if none provided
DOCUMENTS="${@:-doc1.txt doc2.txt doc3.txt}"

# Run with timeout to prevent stalling
timeout 30 cargo run --release --bin entity-recognition-app -- $DOCUMENTS || {
    if [ $? -eq 124 ]; then
        echo ""
        echo "⚠️  Command timed out after 30 seconds"
        echo "   This may indicate a shutdown issue. Check logs above."
    fi
    exit $?
}

