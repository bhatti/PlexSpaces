#!/bin/bash
# Run distributed tests (requires running gRPC servers)

set -e

echo "========================================="
echo "Matrix Multiply - Distributed Tests"
echo "========================================="
echo ""
echo "⚠️  NOTE: This will start multiple gRPC servers"
echo "    Make sure ports 9101-9203 are available"
echo ""

# Run distributed tests
echo "Running distributed tests (2 and 3 node configurations)..."
cargo test --test distributed_test -- --nocapture --test-threads=1 --ignored

echo ""
echo "========================================="
echo "✅ Distributed Tests Complete"
echo "========================================="
