#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Run script for Durable Actor Example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Durable Actor Example                                      ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build in release mode
echo "Building example..."
cargo build --release --bin durable_actor_example --features sqlite-backend

echo ""
echo "Running example..."
echo ""

# Run the example
cargo run --release --bin durable_actor_example --features sqlite-backend

