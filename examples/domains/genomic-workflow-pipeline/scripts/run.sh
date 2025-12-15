#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Run genomic-workflow-pipeline example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

cd "$EXAMPLE_DIR"

# Parse arguments
NUM_READS="${1:-1000}"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Genomic Workflow Pipeline Example                      ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo
echo "Configuration:"
echo "  Number of reads: $NUM_READS"
echo

# Build if needed
if [ ! -f "target/release/genomic-workflow-pipeline" ]; then
    echo "Building example..."
    cargo build --release
    echo
fi

# Run the example
echo "Running pipeline..."
timeout 60 cargo run --release -- "$NUM_READS" || {
    echo "⚠ Example timed out or failed"
    exit 1
}

echo
echo "✓ Example completed successfully"
echo

