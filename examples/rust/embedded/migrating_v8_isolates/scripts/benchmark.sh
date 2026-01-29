#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Comprehensive benchmark script for v8_isolates comparison example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$EXAMPLE_DIR/../../.." && pwd)"

cd "$ROOT_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  V8 Isolates vs PlexSpaces Benchmark                          ║"
echo "║  Production-Grade Performance Testing                         ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo

# Build in release mode
echo "📦 Building example in release mode..."
cargo build --release --bin v8_isolates_comparison || {
    echo "❌ Build failed"
    exit 1
}
echo "✅ Build successful"
echo

# Create metrics directory
mkdir -p "$EXAMPLE_DIR/metrics"

# Run benchmarks
echo "🚀 Running comprehensive benchmarks..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

# Run with timeout to prevent hanging
timeout 300 cargo run --release --bin v8_isolates_comparison 2>&1 | tee "$EXAMPLE_DIR/metrics/benchmark_output.txt" || {
    if [ $? -eq 124 ]; then
        echo ""
        echo "⚠️  Benchmark timed out after 5 minutes"
        echo "   This may indicate a performance issue. Check logs above."
    else
        echo "❌ Benchmark execution failed"
        exit 1
    fi
}

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

# Check if metrics file was created
if [ -f "$EXAMPLE_DIR/metrics/benchmark_results.json" ]; then
    echo "✅ Metrics saved to: metrics/benchmark_results.json"
    echo ""
    echo "📊 Metrics Summary:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Extract key metrics using jq if available, otherwise use grep
    if command -v jq &> /dev/null; then
        echo ""
        echo "Control Plane Metrics:"
        jq -r 'select(.test_name == "Control Plane") | "  Throughput: \(.throughput_events_per_sec) events/sec\n  Latency p95: \(.latency_p95_us) μs\n  Latency p99: \(.latency_p99_us) μs\n  Efficiency: \(.efficiency * 100)%\n  Memory: \(.memory_peak_bytes / 1048576) MB"' "$EXAMPLE_DIR/metrics/benchmark_results.json" 2>/dev/null || echo "  (Unable to parse JSON)"
        
        echo ""
        echo "Data Plane Metrics:"
        jq -r 'select(.test_name == "Data Plane") | "  Throughput: \(.throughput_events_per_sec) events/sec\n  Latency p95: \(.latency_p95_us) μs\n  Latency p99: \(.latency_p99_us) μs\n  Efficiency: \(.efficiency * 100)%\n  Memory: \(.memory_peak_bytes / 1048576) MB"' "$EXAMPLE_DIR/metrics/benchmark_results.json" 2>/dev/null || echo "  (Unable to parse JSON)"
    else
        echo "  (Install 'jq' for JSON parsing: brew install jq or apt-get install jq)"
        echo "  Metrics available in: metrics/benchmark_results.json"
    fi
else
    echo "⚠️  Metrics file not found. Check benchmark output above."
fi

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  ✅ Benchmark Complete                                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"















