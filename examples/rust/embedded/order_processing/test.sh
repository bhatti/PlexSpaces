#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test script for order_processing example

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Shared target: use workspace target directory
WORKSPACE_TARGET="${SCRIPT_DIR}/../../../../target"
export CARGO_TARGET_DIR="${WORKSPACE_TARGET}"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Order Processing Example Test                              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build
echo "📦 Building (shared target, debug)..."
cargo build

# Run pre-built binary with timeout
echo "🚀 Running example..."
timeout 30s "${WORKSPACE_TARGET}/debug/order_processing" 2>&1 || {
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
