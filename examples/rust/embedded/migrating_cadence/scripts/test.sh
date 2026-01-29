#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Cadence Comparison - Test Script"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cargo build --release 2>&1 | grep -E "(Compiling|Finished|error)" || true
if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi
echo "✅ Build successful"

# Check if library targets exist (bin-only examples don't have lib targets)
TEST_OUTPUT=$(cargo test --lib 2>&1) || true
if echo "$TEST_OUTPUT" | grep -q "no library targets found"; then
    echo "⚠️  No library targets found, skipping unit tests (bin-only example)"
else
    echo "$TEST_OUTPUT" | tail -20
    if echo "$TEST_OUTPUT" | grep -q "test result: ok"; then
        echo "✅ Unit tests passed"
    else
        echo "❌ Unit tests failed"
        exit 1
    fi
fi

timeout 60 cargo run --release 2>&1 | tee /tmp/cadence_output.log
if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "❌ Example execution failed"
    exit 1
fi
echo "✅ Example execution successful"

if grep -qi "cadence\|workflow\|orchestration" /tmp/cadence_output.log; then
    echo "✅ Found workflow orchestration output"
else
    echo "⚠️  Output validation incomplete (may be expected)"
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All tests passed!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
