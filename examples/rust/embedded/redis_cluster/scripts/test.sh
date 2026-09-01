#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test script for the Redis Cluster example.
# Builds, runs, and validates that all 10 demo steps pass.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(dirname "$SCRIPT_DIR")"
ROOT_DIR="$(cd "$EXAMPLE_DIR/../../../../" && pwd)"
TARGET_DIR="$ROOT_DIR/target"

export CARGO_TARGET_DIR="$TARGET_DIR"

echo "════════════════════════════════════════════════════════════"
echo "  Redis Cluster — Build & Test"
echo "════════════════════════════════════════════════════════════"
echo

# Check ports 8091 and 8093 are free (embedded test starts its own nodes there)
for port in 8091 8093; do
    if lsof -i ":$port" -n -P 2>/dev/null | grep -q LISTEN; then
        echo "  ERROR: port $port is already in use."
        echo "  Stop any running plexspaces node on that port before running this test."
        exit 1
    fi
done

# Build
echo "Building..."
cd "$EXAMPLE_DIR"
cargo build --bin redis_cluster 2>&1 | tail -5
echo "  ✓ Build succeeded"
echo

# Run and capture output
echo "Running demo..."
OUTPUT=$(cargo run --bin redis_cluster 2>&1)
echo "$OUTPUT"
echo

# Validate each step passed
FAILED=0

check() {
    local label="$1"
    local pattern="$2"
    if echo "$OUTPUT" | grep -q "$pattern"; then
        echo "  ✓ $label"
    else
        echo "  ✗ FAIL: $label (pattern: '$pattern' not found)"
        FAILED=$((FAILED + 1))
    fi
}

echo "Validating output..."
check "Cluster setup"           "Cluster ready"
check "PONG response"           "PING"
check "Basic operations"        "Basic operations"
check "INCR operations"         "INCR operations"
check "Key expiry"              "Expiry"
check "Transactions"            "Transactions"
check "Virtual Actor"           "Virtual Actor"
check "Replication broadcast"   "broadcast_shard_group"
check "WAIT scatter-gather"     "scatter_gather"
check "DBSIZE reduce"           "reduce"
check "KEYS map"                "map + concat"
check "Snapshot"                "parallel map"
check "Multi-node"              "Multi-Node"
check "Throughput benchmark"    "Throughput Benchmark"
check "SET throughput"          "SET/sec"
check "Latency p50"             "p50"
check "Example complete"        "Example Complete"

echo
if [ $FAILED -eq 0 ]; then
    echo "════════════════════════════════════════════════════════════"
    echo "  ALL TESTS PASSED"
    echo "════════════════════════════════════════════════════════════"
    exit 0
else
    echo "════════════════════════════════════════════════════════════"
    echo "  $FAILED TEST(S) FAILED"
    echo "════════════════════════════════════════════════════════════"
    exit 1
fi
