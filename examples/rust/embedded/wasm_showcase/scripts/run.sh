#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Quick start script for WASM showcase example

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$EXAMPLE_DIR"

echo "🚀 Building WASM Showcase..."
cargo build --release

echo ""
echo "📦 Running WASM Showcase..."
echo ""

# Run with all demos by default, or pass arguments
if [ $# -eq 0 ]; then
    cargo run --release
else
    cargo run --release -- "$@"
fi

