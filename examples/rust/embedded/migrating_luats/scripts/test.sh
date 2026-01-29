#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "LuaTS Comparison - Test Script"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

cargo build --release 2>&1 | grep -E "(Compiling|Finished|error)" || true
if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi
echo "✅ Build successful"

cargo test --lib 2>&1 | tail -20
if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "❌ Unit tests failed"
    exit 1
fi
echo "✅ Unit tests passed"

timeout 60 cargo run --release 2>&1 | tee /tmp/luats_output.log
if [ ${PIPESTATUS[0]} -ne 0 ]; then
    echo "❌ Example execution failed"
    exit 1
fi
echo "✅ Example execution successful"

if grep -q "LuaTS" /tmp/luats_output.log; then
    echo "✅ Found event-driven coordination output"
else
    echo "❌ Missing event-driven coordination output"
    exit 1
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ All tests passed!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
