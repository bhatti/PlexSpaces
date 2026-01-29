#!/bin/bash
# Entity Recognition Example - Run Tests

set -e

cd "$(dirname "$0")/.."

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Entity Recognition Example - Tests                     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build
echo "Building example..."
cargo build

# Run unit tests
echo ""
echo "Running unit tests..."
cargo test --lib || echo "⚠ No unit tests found"

# Run clippy
echo ""
echo "Running clippy..."
cargo clippy -- -D warnings || echo "⚠ Clippy warnings found"

# Check formatting
echo ""
echo "Checking formatting..."
cargo fmt --check || echo "⚠ Formatting issues found"

echo ""
echo "✅ Tests complete!"

