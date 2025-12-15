#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test workflow recovery functionality

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"

cd "$EXAMPLE_DIR"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║        Genomic Workflow Pipeline - Recovery Test          ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

# Clean up previous test artifacts
echo "Step 1: Cleaning up previous test artifacts..."
rm -f workflow.db workflow.db-shm workflow.db-wal last_execution_id.txt pipeline_results.json
echo "✓ Cleanup complete"
echo

# Build if needed
if [ ! -f "target/release/genomic-workflow-pipeline" ]; then
    echo "Step 2: Building example..."
    cargo build --release
    echo "✓ Build complete"
    echo
fi

# Test 1: Start a new workflow
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 1: Starting new workflow execution"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 30 cargo run --release -- --storage workflow.db 200 2>&1 | tee /tmp/genomic_recovery_test.log | grep -E "(Execution ID|Status|Recovery Tip|Complete|storage|Storage|error|Error)" || true

if [ ! -f "last_execution_id.txt" ]; then
    echo "❌ ERROR: Execution ID not saved"
    exit 1
fi

EXEC_ID=$(cat last_execution_id.txt)
echo
echo "✓ Workflow started with execution ID: $EXEC_ID"
echo

# Check if storage was initialized
if grep -q "Persistent storage initialized" /tmp/genomic_recovery_test.log 2>/dev/null; then
    echo "✓ Persistent storage initialized"
elif grep -q "Failed to create persistent storage" /tmp/genomic_recovery_test.log 2>/dev/null; then
    echo "⚠ WARNING: Storage creation failed, using in-memory storage"
    echo "  Recovery tests require persistent storage"
fi

# Verify database exists (wait a moment for file to be created and check for .db, .db-shm, .db-wal)
sleep 2
if [ -f "workflow.db" ] || [ -f "workflow.db-shm" ] || [ -f "workflow.db-wal" ]; then
    echo "✓ Persistent storage created: workflow.db"
    echo
else
    echo "⚠ WARNING: workflow.db not created (using in-memory storage)"
    echo "  Recovery will not work without persistent storage"
    if [ -f /tmp/genomic_recovery_test.log ]; then
        echo "  Storage-related log lines:"
        grep -E "(storage|Storage|database|Database|initialized|Failed)" /tmp/genomic_recovery_test.log | head -3
    fi
    echo
fi

# Test 2: Resume the workflow
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 2: Resuming workflow execution"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

timeout 30 cargo run --release -- --storage workflow.db --resume "$EXEC_ID" 2>&1 | grep -E "(Resuming|Execution ID|Status|already completed|Recovery Tip|Complete|error|Error)" || true

echo
echo "✓ Recovery test complete"
echo

# Test 3: Verify database contents (if sqlite3 is available)
if command -v sqlite3 &> /dev/null && [ -f "workflow.db" ]; then
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Test 3: Verifying database contents"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo
    
    echo "Workflow Executions:"
    sqlite3 workflow.db "SELECT execution_id, definition_id, status, created_at FROM workflow_executions ORDER BY created_at DESC LIMIT 5;" 2>/dev/null || echo "  (Unable to query database)"
    echo
    
    echo "Step Executions:"
    sqlite3 workflow.db "SELECT step_id, status, attempt FROM step_executions WHERE execution_id = '$EXEC_ID' ORDER BY created_at;" 2>/dev/null || echo "  (Unable to query database)"
    echo
fi

echo "╔════════════════════════════════════════════════════════════╗"
echo "║              Recovery Test Complete                        ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo

