#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test genomic-workflow-pipeline locally (without Docker)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$EXAMPLE_DIR/../.." && pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║        Genomic Workflow Pipeline - Local Test             ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

# Parse arguments
NUM_READS="${1:-1000}"

echo "Configuration:"
echo "  Project root:   $PROJECT_ROOT"
echo "  Example dir:    $EXAMPLE_DIR"
echo "  Number of reads: $NUM_READS"
echo

# Build the example
echo "Building genomic-workflow-pipeline..."
cd "$EXAMPLE_DIR"
cargo build --release
echo "✓ Build complete"
echo

# Run the pipeline
echo "Running pipeline with $NUM_READS reads..."
time cargo run --release -- "$NUM_READS"
echo

# Check results
if [ -f "pipeline_results.json" ]; then
    echo "✓ Results saved to pipeline_results.json"
    echo
    echo "Metrics Summary:"
    cat pipeline_results.json | grep -E '"(total_reads|qc_passed|variants_called|granularity_ratio|efficiency_percent)"' | sed 's/^/  /'
    echo
else
    echo "⚠ No results file found"
fi

echo "═══════════════════════════════════════════════════════════"
echo "✓ Local test complete"
echo
