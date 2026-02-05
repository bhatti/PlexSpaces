#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Session Manager App – test script (uses debug build per project convention)

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

if ! command -v cargo &> /dev/null; then
    echo "cargo not found"
    exit 1
fi

echo "Building session_manager (debug)..."
cargo build 2>&1 | tail -5
echo "Running session_manager (timeout 12s)..."
timeout 12s cargo run --bin session_manager 2>&1 | tee /tmp/session_manager_out.txt || true
echo ""
echo "Checking output..."
grep -E "Session Manager|Node started|actor spawned|Timers registered|heartbeat|idle_timeout|complete" /tmp/session_manager_out.txt || true
echo "Done."
