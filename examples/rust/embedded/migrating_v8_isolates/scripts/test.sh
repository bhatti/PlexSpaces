#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for v8_isolates comparison example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$EXAMPLE_DIR/../../.." && pwd)"

cd "$ROOT_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  Testing V8 Isolates Comparison Example                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo

# Build
echo "📦 Building example..."
cargo build --release --bin v8_isolates_comparison || {
    echo "❌ Build failed"
    exit 1
}
echo "✅ Build successful"
echo

# Run tests
echo "🧪 Running tests..."
cargo test --package v8-isolates-comparison || {
    echo "❌ Tests failed"
    exit 1
}
echo "✅ Tests passed"
echo

# Run example
echo "🚀 Running example..."
cargo run --release --bin v8_isolates_comparison || {
    echo "❌ Example execution failed"
    exit 1
}
echo "✅ Example execution successful"
echo

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  ✅ All tests passed                                           ║"
echo "╚════════════════════════════════════════════════════════════════╝"















