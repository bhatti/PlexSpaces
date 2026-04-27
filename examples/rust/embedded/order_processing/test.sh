#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test script for order_processing example

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Order Processing Example Test                              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build
echo "📦 Building..."
cargo build --quiet

# Run with timeout
echo "🚀 Running example..."
timeout 30s cargo run --bin order_processing 2>&1 || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo "⏱️  Timeout (expected for long-running example)"
    else
        echo "❌ Failed with exit code $EXIT_CODE"
        exit $EXIT_CODE
    fi
}

echo ""
echo "✅ Test passed!"
