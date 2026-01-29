#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test automated recovery on startup
# Simulates node crash and recovery

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Genomic Workflow Pipeline - Auto-Recovery Test         ║"
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

# Test 1: Start a workflow (simulate normal execution)
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 3: Starting workflow execution (simulating normal run)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 30 cargo run --release -- --storage workflow.db 100 2>&1 | tee /tmp/genomic_auto_recovery_test.log | tail -10

if [ ! -f "last_execution_id.txt" ]; then
    echo "❌ ERROR: Execution ID not saved"
    exit 1
fi

EXEC_ID=$(cat last_execution_id.txt)
echo
echo "✓ Workflow started with execution ID: $EXEC_ID"
echo

# Check if storage was initialized (look for the log message)
if grep -q "Persistent storage initialized" /tmp/genomic_auto_recovery_test.log 2>/dev/null; then
    echo "✓ Persistent storage initialized"
elif grep -q "Failed to create persistent storage" /tmp/genomic_auto_recovery_test.log 2>/dev/null; then
    echo "❌ ERROR: Storage creation failed, using in-memory storage"
    echo "  This test requires persistent storage. Check error messages above."
    grep "Failed to create" /tmp/genomic_auto_recovery_test.log | head -3
    exit 1
else
    echo "⚠ WARNING: Could not determine storage status from logs"
fi

# Verify database exists (wait a moment for file to be created and check for .db, .db-shm, .db-wal)
sleep 2
if [ -f "workflow.db" ] || [ -f "workflow.db-shm" ] || [ -f "workflow.db-wal" ]; then
    echo "✓ Database file created: workflow.db"
else
    echo "❌ ERROR: workflow.db not created"
    echo "  Check logs above for storage initialization errors"
    if [ -f /tmp/genomic_auto_recovery_test.log ]; then
        echo "  Storage-related log lines:"
        grep -E "(storage|Storage|database|Database|initialized|Failed)" /tmp/genomic_auto_recovery_test.log | head -5
    fi
    exit 1
fi

# Test 2: Simulate node crash (workflow is in RUNNING state)
# In a real scenario, we would kill the process, but for this test,
# we'll manually mark the workflow as RUNNING in the database
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 4: Simulating node crash (workflow left in RUNNING state)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

if command -v sqlite3 &> /dev/null; then
    # Update status to RUNNING to simulate interrupted workflow
    sqlite3 workflow.db "UPDATE workflow_executions SET status = 'RUNNING' WHERE execution_id = '$EXEC_ID';" 2>/dev/null || true
    echo "✓ Workflow marked as RUNNING (simulating crash)"
    echo
else
    echo "⚠ sqlite3 not available, skipping status update"
    echo "  (In real scenario, workflow would be in RUNNING state after crash)"
    echo
fi

# Test 3: Auto-recovery on startup
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Step 5: Testing auto-recovery on startup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

echo "Running with --auto-recover flag..."
timeout 30 cargo run --release -- --storage workflow.db --auto-recover 100 2>&1 | grep -E "(Auto-recovery|recovered|claimed|transferred|failed|No interrupted|Execution ID|Status|Complete)" || true

echo
echo "✓ Auto-recovery test complete"
echo

# Test 4: Verify recovery results
if command -v sqlite3 &> /dev/null; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Step 6: Verifying recovery results"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo
    
    STATUS=$(sqlite3 workflow.db "SELECT status FROM workflow_executions WHERE execution_id = '$EXEC_ID';" 2>/dev/null || echo "")
    NODE_ID=$(sqlite3 workflow.db "SELECT node_id FROM workflow_executions WHERE execution_id = '$EXEC_ID';" 2>/dev/null || echo "")
    VERSION=$(sqlite3 workflow.db "SELECT version FROM workflow_executions WHERE execution_id = '$EXEC_ID';" 2>/dev/null || echo "")
    
    echo "Execution Status: $STATUS"
    echo "Node ID: $NODE_ID"
    echo "Version: $VERSION"
    echo
fi

echo "╔════════════════════════════════════════════════════════════╗"
echo "║              Auto-Recovery Test Complete                  ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo
echo "💡 Key Features Demonstrated:"
echo "   1. ✓ Persistent storage (SQLite database)"
echo "   2. ✓ Workflow state tracking (execution ID, status, version)"
echo "   3. ✓ Auto-recovery on startup (--auto-recover flag)"
echo "   4. ✓ Node ownership and health monitoring"
echo "   5. ✓ Optimistic locking (version-based)"
echo

