#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-simple-actor"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/abstractions"
OUTPUT_WASM="$TARGET_DIR/abstractions_actor.wasm"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

if [ ! -f "node_modules/.bin/tsc" ]; then
  npm install --no-audit --no-fund
fi

if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
fi

./node_modules/.bin/tsc -p .
node build-bundle.mjs

if [ -f "node_modules/.bin/jco" ]; then
  JCO="./node_modules/.bin/jco"
elif command -v jco >/dev/null 2>&1; then
  JCO="jco"
else
  npm install --no-audit --no-fund @bytecodealliance/jco @bytecodealliance/componentize-js
  JCO="./node_modules/.bin/jco"
fi

$JCO componentize abstractions_actor_bundle.mjs --wit "$WIT_DIR" -o "$OUTPUT_WASM" --disable all
echo "Built TypeScript abstractions WASM component at $OUTPUT_WASM"
