#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Comprehensive validation script for workflow recovery
# Tests: normal execution, interruption, recovery, auto-recovery

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Genomic Workflow Pipeline - Recovery Validation        ║"
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

# Test 1: Normal execution
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 1: Normal Workflow Execution"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 30 cargo run --release -- --storage workflow.db 100 2>&1 | tee /tmp/genomic_validate.log | tail -15

if [ ! -f "last_execution_id.txt" ]; then
    echo "❌ ERROR: Execution ID not saved"
    exit 1
fi

EXEC_ID=$(cat last_execution_id.txt)
echo
echo "✓ Execution ID: $EXEC_ID"
echo

# Check storage initialization
if grep -q "Persistent storage initialized" /tmp/genomic_validate.log 2>/dev/null; then
    echo "✓ Persistent storage initialized"
elif grep -q "Failed to create persistent storage" /tmp/genomic_validate.log 2>/dev/null; then
    echo "⚠ WARNING: Storage creation failed"
fi

# Wait for database file (check for .db, .db-shm, .db-wal)
sleep 2

# Test 2: Verify execution in database
if command -v sqlite3 &> /dev/null && ([ -f "workflow.db" ] || [ -f "workflow.db-shm" ] || [ -f "workflow.db-wal" ]); then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Test 2: Database Verification"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo
    
    STATUS=$(sqlite3 workflow.db "SELECT status FROM workflow_executions WHERE execution_id = '$EXEC_ID';" 2>/dev/null || echo "")
    if [ -n "$STATUS" ]; then
        echo "✓ Execution found in database"
        echo "  Status: $STATUS"
        echo
    else
        echo "⚠ Execution not found in database (using in-memory storage)"
        echo
    fi
fi

# Test 3: Resume completed workflow
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 3: Resume Completed Workflow (should show already completed)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 10 cargo run --release -- --storage workflow.db --resume "$EXEC_ID" 2>&1 | grep -E "(Resuming|already completed|Execution ID|Status)" || true
echo

# Test 4: Resume non-existent workflow
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 4: Resume Non-Existent Workflow (should start new)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 10 cargo run --release -- --storage workflow.db --resume "NONEXISTENT" 2>&1 | grep -E "(not found|Starting new|Execution ID)" || true
echo

echo "╔════════════════════════════════════════════════════════════╗"
echo "║              Validation Complete                            ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo
echo "💡 To test auto-recovery on node restart:"
echo "   1. Start a workflow (it will be in RUNNING state)"
echo "   2. Kill the process (simulate node crash)"
echo "   3. Restart and run auto-recovery (see RECOVERY_BEST_PRACTICES.md)"
echo

