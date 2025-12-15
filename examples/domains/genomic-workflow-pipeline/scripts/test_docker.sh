#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test genomic-workflow-pipeline in Docker (single container)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$EXAMPLE_DIR/../.." && pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║        Genomic Workflow Pipeline - Docker Test            ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

# Parse arguments
NUM_READS="${1:-10000}"
IMAGE_NAME="plexspaces/genomic-workflow-pipeline:latest"

echo "Configuration:"
echo "  Project root:   $PROJECT_ROOT"
echo "  Docker image:   $IMAGE_NAME"
echo "  Number of reads: $NUM_READS"
echo

# Build Docker image
echo "Building Docker image..."
cd "$PROJECT_ROOT"
docker build -f examples/genomic-workflow-pipeline/Dockerfile -t "$IMAGE_NAME" .
echo "✓ Docker image built"
echo

# Run container
echo "Running pipeline in Docker container..."
docker run --rm \
    -e RUST_LOG=info \
    "$IMAGE_NAME" \
    genomic-workflow-pipeline "$NUM_READS"
echo

echo "═══════════════════════════════════════════════════════════"
echo "✓ Docker test complete"
echo
echo "To run with different parameters:"
echo "  $0 <num_reads>"
echo
echo "Examples:"
echo "  $0 1000     # Process 1K reads"
echo "  $0 10000    # Process 10K reads"
echo "  $0 100000   # Process 100K reads (stress test)"
echo
