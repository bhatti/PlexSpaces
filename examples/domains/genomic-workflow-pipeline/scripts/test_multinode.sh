#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test genomic-workflow-pipeline in multi-node Docker Compose cluster

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"
PROJECT_ROOT="$(cd "$EXAMPLE_DIR/../.." && pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║      Genomic Workflow Pipeline - Multi-Node Test          ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

echo "Configuration:"
echo "  Project root:    $PROJECT_ROOT"
echo "  Example dir:     $EXAMPLE_DIR"
echo "  Docker Compose:  docker-compose.yml"
echo

# Clean up previous results
echo "Cleaning up previous results..."
rm -rf "$EXAMPLE_DIR/results"
mkdir -p "$EXAMPLE_DIR/results"

# Build Docker image
echo "Building Docker image..."
cd "$PROJECT_ROOT"
docker build -f examples/genomic-workflow-pipeline/Dockerfile -t plexspaces/genomic-workflow-pipeline:latest .
echo "✓ Docker image built"
echo

# Run multi-node cluster
echo "Starting multi-node cluster..."
echo "  Worker 1: 1,000 reads"
echo "  Worker 2: 2,000 reads"
echo "  Worker 3: 5,000 reads"
echo "  Worker 4: 10,000 reads (stress test)"
echo

cd "$EXAMPLE_DIR"
docker-compose up --abort-on-container-exit
echo

# Aggregate metrics
echo "═══════════════════════════════════════════════════════════"
echo "                    CLUSTER METRICS                         "
echo "═══════════════════════════════════════════════════════════"
echo

# Get logs from all workers
echo "Worker Results:"
echo
for worker in worker-1 worker-2 worker-3 worker-4; do
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Container: genomic-$worker"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    docker logs genomic-$worker 2>&1 | grep -A 15 "PIPELINE RESULTS" || echo "  (No results captured)"
    echo
done

# Calculate aggregate statistics
echo "═══════════════════════════════════════════════════════════"
echo "                  AGGREGATE STATISTICS                      "
echo "═══════════════════════════════════════════════════════════"
echo

# Extract metrics from logs
TOTAL_READS=0
TOTAL_VARIANTS=0
MIN_RATIO=999999
MAX_RATIO=0
SUM_RATIO=0
COUNT=0

for worker in worker-1 worker-2 worker-3 worker-4; do
    # Extract total reads
    READS=$(docker logs genomic-$worker 2>&1 | grep "Total reads:" | awk '{print $NF}' || echo "0")
    TOTAL_READS=$((TOTAL_READS + READS))

    # Extract variants
    VARIANTS=$(docker logs genomic-$worker 2>&1 | grep "Variants called:" | awk '{print $NF}' || echo "0")
    TOTAL_VARIANTS=$((TOTAL_VARIANTS + VARIANTS))

    # Extract granularity ratio
    RATIO=$(docker logs genomic-$worker 2>&1 | grep "Granularity Ratio" | awk '{print $(NF-1)}' | tr -d '×' || echo "0")
    if [ -n "$RATIO" ] && [ "$RATIO" != "0" ]; then
        SUM_RATIO=$(echo "$SUM_RATIO + $RATIO" | bc -l)
        COUNT=$((COUNT + 1))

        # Check min/max
        if [ $(echo "$RATIO < $MIN_RATIO" | bc -l) -eq 1 ]; then
            MIN_RATIO=$RATIO
        fi
        if [ $(echo "$RATIO > $MAX_RATIO" | bc -l) -eq 1 ]; then
            MAX_RATIO=$RATIO
        fi
    fi
done

# Calculate average ratio
if [ "$COUNT" -gt 0 ]; then
    AVG_RATIO=$(echo "scale=1; $SUM_RATIO / $COUNT" | bc -l)
else
    AVG_RATIO="0"
fi

echo "  Total Reads Processed:    $TOTAL_READS"
echo "  Total Variants Called:    $TOTAL_VARIANTS"
echo
echo "  Granularity Metrics:"
echo "    Average Ratio:          ${AVG_RATIO}×"
echo "    Min Ratio:              ${MIN_RATIO}×"
echo "    Max Ratio:              ${MAX_RATIO}×"
echo

# Evaluate cluster performance
TARGET_RATIO=100.0
if [ $(echo "$AVG_RATIO >= $TARGET_RATIO" | bc -l) -eq 1 ]; then
    echo "  Status:                 ✅ OPTIMAL (avg ratio >= ${TARGET_RATIO}×)"
    echo "  Analysis:               Excellent granularity across all workers!"
elif [ $(echo "$AVG_RATIO >= 10.0" | bc -l) -eq 1 ]; then
    echo "  Status:                 ⚠  ACCEPTABLE (avg ratio >= 10×, < ${TARGET_RATIO}×)"
    echo "  Recommendation:         Consider batching more work per task."
else
    echo "  Status:                 ❌ SUBOPTIMAL (avg ratio < 10×)"
    echo "  Recommendation:         Increase batch size significantly!"
fi
echo

echo "═══════════════════════════════════════════════════════════"
echo "✓ Multi-node test complete"
echo

# Clean up
echo "Cleaning up containers..."
docker-compose down
echo "✓ Cleanup complete"
echo

echo "To run again:"
echo "  cd $EXAMPLE_DIR"
echo "  ./scripts/test_multinode.sh"
echo
