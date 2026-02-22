#!/bin/bash
# Test script for genomics_pipeline example
set -e

echo "Building genomics_pipeline..."
cargo build -p genomics-pipeline

echo ""
echo "Running genomics_pipeline..."
cargo run -p genomics-pipeline

echo ""
echo "✅ Test passed!"
