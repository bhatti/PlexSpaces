#!/bin/bash
# Test script for supervision tree example

set -e

echo "Running supervision tree example tests..."

# Run the example
cargo run --bin supervision_tree

echo "✅ Supervision tree example tests passed"
