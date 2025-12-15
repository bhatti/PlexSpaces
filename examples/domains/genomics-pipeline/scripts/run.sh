#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Genomics DNA Sequencing Pipeline Example                  ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build the example
echo "Building genomics-pipeline..."
cargo build --release

echo ""
echo "Running genomics-pipeline example..."
echo ""

# Run with sample arguments if provided, otherwise run without
if [ $# -ge 2 ]; then
    cargo run --release --bin genomics-pipeline -- --sample-id "$1" --fastq "$2"
else
    cargo run --release --bin genomics-pipeline
fi

