#!/usr/bin/env bash
# Build log processor actor WASM: TypeScript → JS → ESM bundle → WASM component
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-simple-actor"
OUTPUT_WASM="$SCRIPT_DIR/log_processor_actor.wasm"

cd "$SCRIPT_DIR"

echo "Building TypeScript log processor (V8 Isolates style) → WASM..."
echo ""

if [ ! -f "node_modules/.bin/tsc" ]; then
  echo "  Installing dependencies..."
  npm install --no-audit --no-fund
fi

if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
fi

./node_modules/.bin/tsc -p .
if [ ! -f "log_processor_actor.js" ]; then
  echo "  ERROR: log_processor_actor.js not produced"
  exit 1
fi
echo "  ✓ log_processor_actor.js"

if command -v node &>/dev/null; then
  node build-bundle.mjs
else
  echo "  ERROR: node not found"
  exit 1
fi

if [ ! -d "$WIT_DIR" ] || [ ! -f "$WIT_DIR/world.wit" ]; then
  echo "  ERROR: WIT dir not found: $WIT_DIR"
  exit 1
fi

if [ -f "node_modules/.bin/jco" ]; then
  JCO="./node_modules/.bin/jco"
elif command -v jco &>/dev/null; then
  JCO="jco"
else
  echo "  Installing jco and componentize-js..."
  npm install --no-audit --no-fund @bytecodealliance/jco @bytecodealliance/componentize-js
  JCO="./node_modules/.bin/jco"
fi

if ! $JCO componentize log_processor_actor_bundle.mjs --wit "$WIT_DIR" -o "$OUTPUT_WASM" --disable all 2>&1; then
  echo "  ERROR: jco componentize failed (see above)."
  exit 1
fi

SIZE=$(ls -lh "$OUTPUT_WASM" | awk '{print $5}')
echo "  ✓ $OUTPUT_WASM ($SIZE)"
echo ""
echo "Build complete. Run ./test.sh [HTTP_PORT] to deploy and test."
