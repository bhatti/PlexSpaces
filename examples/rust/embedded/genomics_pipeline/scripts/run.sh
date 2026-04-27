#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
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
cargo build

echo ""
echo "Running genomics-pipeline example..."
echo ""

# Run with sample arguments if provided, otherwise use bundled sample data.
if [ $# -ge 2 ]; then
    cargo run -- --sample-id "$1" --fastq "$2"
else
    cargo run -- --sample-id SAMPLE001 --fastq test_data/sample001.fastq
fi
