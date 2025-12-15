#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Basic test to verify database creation and workflow execution

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Genomic Workflow Pipeline - Basic Test                 ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

# Clean up
echo "Step 1: Cleaning up..."
rm -f workflow.db workflow.db-shm workflow.db-wal last_execution_id.txt pipeline_results.json
echo "✓ Cleanup complete"
echo

# Build
if [ ! -f "target/release/genomic-workflow-pipeline" ]; then
    echo "Step 2: Building..."
    cargo build --release
    echo "✓ Build complete"
    echo
fi

# Test: Run with small number of reads
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 3: Running workflow (10 reads)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 30 cargo run --release -- --storage workflow.db 10 2>&1 | tee /tmp/genomic_basic_test.log

# Check results
if [ ! -f "last_execution_id.txt" ]; then
    echo "❌ ERROR: Execution ID not saved"
    echo "  Last 20 lines of output:"
    tail -20 /tmp/genomic_basic_test.log
    exit 1
fi

EXEC_ID=$(cat last_execution_id.txt)
echo
echo "✓ Execution ID: $EXEC_ID"
echo

# Check database
sleep 1
if [ -f "workflow.db" ] || [ -f "workflow.db-shm" ] || [ -f "workflow.db-wal" ]; then
    echo "✓ Database file created"
    if command -v sqlite3 &> /dev/null; then
        echo
        echo "Database contents:"
        sqlite3 workflow.db "SELECT execution_id, status FROM workflow_executions WHERE execution_id = '$EXEC_ID';" 2>/dev/null || echo "  (Unable to query)"
    fi
else
    echo "⚠ WARNING: Database file not found"
    echo "  Check logs for storage initialization errors:"
    grep -E "(storage|Storage|database|Database|Failed|error|Error)" /tmp/genomic_basic_test.log | head -5
fi

echo
echo "╔════════════════════════════════════════════════════════════╗"
echo "║              Basic Test Complete                           ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

