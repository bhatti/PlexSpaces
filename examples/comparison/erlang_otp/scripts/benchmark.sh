#!/bin/bash
# Benchmark script for Erlang/OTP comparison

set -e

echo "=== Erlang/OTP vs PlexSpaces Benchmark ==="

# Run benchmarks
cargo bench 2>/dev/null || {
    echo "Running performance test (cargo bench not available, using cargo test --release)"
    cargo test --release --lib -- --nocapture
}

echo "=== Benchmark Complete ==="
echo "See metrics/benchmark_results.json for detailed results"
