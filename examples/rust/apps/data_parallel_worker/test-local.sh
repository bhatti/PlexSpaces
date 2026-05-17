#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Local test script for data_parallel_worker example
# Starts 3 local servers and runs the example against them

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"
TARGET_DIR="$WORKSPACE_ROOT/target"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Local Test: Data Parallel Worker Example"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Kill any existing plexspaces processes to avoid port conflicts
echo "Killing any existing plexspaces processes..."
pkill -9 -f 'plexspaces start' 2>/dev/null || true
pkill -9 -f 'firecracker_multi_tenant' 2>/dev/null || true
pkill -9 -f 'data_parallel_worker' 2>/dev/null || true
sleep 1

# Check for processes using ports 8000, 8001, 8010, 8011, 8020, 8021
for port in 8000 8001 8010 8011 8020 8021; do
    if lsof -ti:$port > /dev/null 2>&1; then
        echo "⚠️  Warning: Port $port is in use, attempting to kill process..."
        lsof -ti:$port | xargs kill -9 2>/dev/null || true
        sleep 1
    fi
done

echo "✅ Cleaned up existing processes"
echo ""

# Check if plexspaces binary exists
PLEXSPACES_BIN="$TARGET_DIR/debug/plexspaces"
if [ ! -f "$PLEXSPACES_BIN" ]; then
    echo "Building plexspaces binary..."
    cd "$WORKSPACE_ROOT"
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build -p plexspaces-cli --bin plexspaces
fi

# Create log directory for server logs (similar to scripts/server.sh)
LOG_DIR="$SCRIPT_DIR/logs"
mkdir -p "$LOG_DIR"

echo "Server log directory: $LOG_DIR"
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo "Cleaning up..."
    # Kill node processes
    kill $NODE1_PID $NODE2_PID $NODE3_PID 2>/dev/null || true
    # Kill any remaining plexspaces processes
    pkill -9 -f 'plexspaces start' 2>/dev/null || true
    pkill -9 -f 'firecracker_multi_tenant' 2>/dev/null || true
    pkill -9 -f 'data_parallel_worker' 2>/dev/null || true
    sleep 1
    echo "✅ Cleanup complete"
    echo "Server logs available at:"
    echo "  Node1: $LOG_DIR/node1.log"
    echo "  Node2: $LOG_DIR/node2.log"
    echo "  Node3: $LOG_DIR/node3.log"
}

trap cleanup EXIT

# Set up environment variables for local testing
# Note: Default release.yaml in workspace root disables auth, so no need to set PLEXSPACES_DISABLE_AUTH
# DEBUG logging for PlexSpaces crates
export RUST_LOG="${RUST_LOG:-warn,plexspaces_actor=debug,plexspaces_node=debug,plexspaces_services=debug,plexspaces_wasm_runtime=debug,plexspaces_core=debug,plexspaces_application=debug,plexspaces_facet=debug,plexspaces_mailbox=debug,plexspaces_behavior=debug}"

# Use release.yaml from workspace root only (default disables auth)
RELEASE_CONFIG="$WORKSPACE_ROOT/release.yaml"

if [ ! -f "$RELEASE_CONFIG" ]; then
    echo "⚠️  Warning: No release.yaml found at $RELEASE_CONFIG, nodes will use default config"
    RELEASE_CONFIG=""
else
    echo "Using release config: $RELEASE_CONFIG"
fi

# Start node1 on port 8000 (HTTP will be 8001)
echo "Starting node1 on port 8000..."
cd "$WORKSPACE_ROOT"
# Ensure log file exists and is writable before starting node
touch "$LOG_DIR/node1.log" || { echo "ERROR: Cannot create log file: $LOG_DIR/node1.log"; exit 1; }
if [ -n "$RELEASE_CONFIG" ]; then
    echo "Starting node1 with release config: $RELEASE_CONFIG"
    "$PLEXSPACES_BIN" start --node-id "node1" --listen-addr "0.0.0.0:8000" --release-config "$RELEASE_CONFIG" \
        >> "$LOG_DIR/node1.log" 2>&1 &
else
    echo "Starting node1 without release config (using defaults)"
    "$PLEXSPACES_BIN" start --node-id "node1" --listen-addr "0.0.0.0:8000" \
        >> "$LOG_DIR/node1.log" 2>&1 &
fi
NODE1_PID=$!
echo "Node1 PID: $NODE1_PID" | tee -a "$LOG_DIR/node1.log"

# Start node2 on port 8010 (HTTP will be 8011) - using 8010 to avoid conflict with node1's HTTP port
echo "Starting node2 on port 8010..."
touch "$LOG_DIR/node2.log" || { echo "ERROR: Cannot create log file: $LOG_DIR/node2.log"; exit 1; }
if [ -n "$RELEASE_CONFIG" ]; then
    echo "Starting node2 with release config: $RELEASE_CONFIG"
    "$PLEXSPACES_BIN" start --node-id "node2" --listen-addr "0.0.0.0:8010" --release-config "$RELEASE_CONFIG" \
        >> "$LOG_DIR/node2.log" 2>&1 &
else
    echo "Starting node2 without release config (using defaults)"
    "$PLEXSPACES_BIN" start --node-id "node2" --listen-addr "0.0.0.0:8010" \
        >> "$LOG_DIR/node2.log" 2>&1 &
fi
NODE2_PID=$!
echo "Node2 PID: $NODE2_PID" | tee -a "$LOG_DIR/node2.log"

# Start node3 on port 8020 (HTTP will be 8021) - using 8020 to avoid conflicts
echo "Starting node3 on port 8020..."
touch "$LOG_DIR/node3.log" || { echo "ERROR: Cannot create log file: $LOG_DIR/node3.log"; exit 1; }
if [ -n "$RELEASE_CONFIG" ]; then
    echo "Starting node3 with release config: $RELEASE_CONFIG"
    "$PLEXSPACES_BIN" start --node-id "node3" --listen-addr "0.0.0.0:8020" --release-config "$RELEASE_CONFIG" \
        >> "$LOG_DIR/node3.log" 2>&1 &
else
    echo "Starting node3 without release config (using defaults)"
    "$PLEXSPACES_BIN" start --node-id "node3" --listen-addr "0.0.0.0:8020" \
        >> "$LOG_DIR/node3.log" 2>&1 &
fi
NODE3_PID=$!
echo "Node3 PID: $NODE3_PID" | tee -a "$LOG_DIR/node3.log"

echo "Waiting for nodes to start..."
sleep 3

# Verify log files were created and show initial content
echo ""
echo "Checking node startup logs..."
for node_num in 1 2 3; do
    log_file="$LOG_DIR/node${node_num}.log"
    if [ ! -f "$log_file" ]; then
        echo "⚠️  Warning: node${node_num}.log was not created. Node may have failed to start."
    elif [ ! -s "$log_file" ]; then
        echo "⚠️  Warning: node${node_num}.log exists but is empty. Node may have failed to start."
    else
        echo "Node${node_num} log (last 15 lines):"
        tail -15 "$log_file" || echo "  (error reading log file)"
        echo ""
    fi
done

# Check if nodes are running
if ! kill -0 $NODE1_PID 2>/dev/null; then
    echo "❌ Node1 failed to start. Logs:"
    tail -30 "$LOG_DIR/node1.log"
    exit 1
fi

if ! kill -0 $NODE2_PID 2>/dev/null; then
    echo "❌ Node2 failed to start. Logs:"
    tail -30 "$LOG_DIR/node2.log"
    exit 1
fi

if ! kill -0 $NODE3_PID 2>/dev/null; then
    echo "❌ Node3 failed to start. Logs:"
    tail -30 "$LOG_DIR/node3.log"
    exit 1
fi

echo "✅ All nodes started (processes running)"
echo "   Node1: gRPC http://localhost:8000"
echo "   Node2: gRPC http://localhost:8010"
echo "   Node3: gRPC http://localhost:8020"
echo ""
echo "Server logs:"
echo "   Node1: $LOG_DIR/node1.log"
echo "   Node2: $LOG_DIR/node2.log"
echo "   Node3: $LOG_DIR/node3.log"
echo ""
echo "To view logs: tail -f $LOG_DIR/node1.log"
echo ""
echo "Waiting for nodes to initialize (SDK will handle connection retries)..."
sleep 5
echo ""

# Run the example (capture output to log file)
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Running example..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Example output will be written to: $LOG_DIR/example.log"
echo ""
cd "$SCRIPT_DIR"
touch "$LOG_DIR/example.log" || { echo "ERROR: Cannot create log file: $LOG_DIR/example.log"; exit 1; }
CARGO_TARGET_DIR="$TARGET_DIR" cargo run >> "$LOG_DIR/example.log" 2>&1
EXAMPLE_EXIT_CODE=$?

echo ""
if [ $EXAMPLE_EXIT_CODE -eq 0 ]; then
    echo "✅ Example completed successfully!"
else
    echo "❌ Example failed with exit code: $EXAMPLE_EXIT_CODE"
    echo "   Check logs: $LOG_DIR/example.log"
fi
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Log Files Summary"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "   Node1 (primary):  $LOG_DIR/node1.log"
echo "   Node2:            $LOG_DIR/node2.log"
echo "   Node3:            $LOG_DIR/node3.log"
echo "   Example output:   $LOG_DIR/example.log"
echo ""
echo "To view logs:"
echo "   tail -f $LOG_DIR/node1.log    # Node1 logs (where workers run)"
echo "   tail -f $LOG_DIR/example.log  # Example application output"
echo ""
echo "To search for debug messages:"
echo "   grep -E '\[ROUTING DEBUG\]|\[REPLY DEBUG\]|\[ACTOR_SERVICE DEBUG\]' $LOG_DIR/node1.log"
echo "   grep -E 'sender_id_empty|Temporary sender|Looking up' $LOG_DIR/node1.log"
