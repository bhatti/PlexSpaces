#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Run script for Process Groups (Pub/Sub) Example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Process Groups (Pub/Sub) - Example                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build in release mode
echo "Building example..."
cargo build --release --bin process_groups_pubsub

echo ""
echo "Running example..."
echo ""

# Run the example
cargo run --release --bin process_groups_pubsub

