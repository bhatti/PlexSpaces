#!/bin/bash
# Run E2E tests with detailed metrics output

set -e

echo "========================================="
echo "Heat Diffusion - E2E Tests with Metrics"
echo "========================================="
echo ""

# Run E2E tests with full output
cargo test --test e2e_test -- --nocapture --test-threads=1

echo ""
echo "========================================="
echo "✅ E2E Tests Complete"
echo "========================================="
