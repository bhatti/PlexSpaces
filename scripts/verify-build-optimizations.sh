#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Verify build optimizations are working

set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

echo "═══════════════════════════════════════════════════════"
echo "  Build Optimization Verification"
echo "═══════════════════════════════════════════════════════"
echo ""

# Check .cargo/config.toml
echo "1. Checking .cargo/config.toml..."
if [ -f ".cargo/config.toml" ]; then
    echo "   ✓ .cargo/config.toml exists"
    JOBS=$(grep "^jobs = " .cargo/config.toml | sed 's/.*jobs = \([0-9]*\).*/\1/' || echo "not found")
    INCREMENTAL=$(grep "^incremental = " .cargo/config.toml | head -1 | grep -q "true" && echo "true" || echo "false")
    echo "   - Default jobs: $JOBS"
    echo "   - Incremental: $INCREMENTAL"
else
    echo "   ✗ .cargo/config.toml not found"
fi
echo ""

# Check incremental directory
echo "2. Checking incremental build artifacts..."
if [ -d "target/debug/incremental" ]; then
    CRATE_COUNT=$(find target/debug/incremental -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l || echo "0")
    echo "   ✓ Incremental directory exists"
    echo "   - Crates with incremental artifacts: $CRATE_COUNT"
    if [ "$CRATE_COUNT" -gt 0 ]; then
        echo "   ✓ Incremental builds are working!"
    else
        echo "   ⚠ No incremental artifacts yet (run 'make build' first)"
    fi
else
    echo "   ⚠ Incremental directory not found (run 'make build' first)"
fi
echo ""

# Check cargo version
echo "3. Cargo version and features..."
CARGO_VERSION=$(cargo --version 2>/dev/null || echo "not found")
echo "   $CARGO_VERSION"
echo ""

# Check if concurrent builds work
echo "4. Testing concurrent compilation..."
echo "   Run: cargo build --jobs 4 --message-format=short"
echo "   Should see multiple 'Compiling' messages at once"
echo ""

# Summary
echo "═══════════════════════════════════════════════════════"
echo "  Summary"
echo "═══════════════════════════════════════════════════════"
echo ""
echo "To verify optimizations:"
echo "1. Run: make build"
echo "2. Check output shows 'Compiling' messages (progress visible)"
echo "3. Run again: make build (should be faster - incremental)"
echo "4. Check: ls -la target/debug/incremental/ (should have crate dirs)"
echo ""
