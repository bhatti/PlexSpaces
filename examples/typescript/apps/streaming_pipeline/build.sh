#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/streaming_actor.wasm"
ESBUILD_STAMP="$SCRIPT_DIR/node_modules/.esbuild-platform"

cd "$SCRIPT_DIR"

echo "Building Streaming Pipeline (TypeScript WASM)..."

CURRENT_NODE_PLATFORM="$(node -p '`${process.platform}-${process.arch}`')"
INSTALLED_NODE_PLATFORM=""
if [ -f "$ESBUILD_STAMP" ]; then
  INSTALLED_NODE_PLATFORM="$(cat "$ESBUILD_STAMP" 2>/dev/null || true)"
fi

if [ ! -f "node_modules/.bin/tsc" ] || [ ! -f "node_modules/.bin/jco" ] || [ "$INSTALLED_NODE_PLATFORM" != "$CURRENT_NODE_PLATFORM" ]; then
  echo "  Installing dependencies..."
  if [ "$INSTALLED_NODE_PLATFORM" != "$CURRENT_NODE_PLATFORM" ] && [ -n "$INSTALLED_NODE_PLATFORM" ]; then
    echo "  Detected Node platform change: $INSTALLED_NODE_PLATFORM -> $CURRENT_NODE_PLATFORM"
  fi
  if [ -f "package-lock.json" ]; then
    npm ci --no-audit --no-fund
  else
    npm install --no-audit --no-fund
  fi
  printf '%s\n' "$CURRENT_NODE_PLATFORM" > "$ESBUILD_STAMP"
fi

if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
  if [ -d "$REPO_ROOT/sdks/typescript/dist" ] && [ -d "node_modules/@plexspaces/sdk" ]; then
    cp -r "$REPO_ROOT/sdks/typescript/dist/." "node_modules/@plexspaces/sdk/dist/"
  fi
fi

./node_modules/.bin/tsc -p .
node build-bundle.mjs

if [ -f "node_modules/.bin/jco" ]; then
  JCO="./node_modules/.bin/jco"
elif command -v jco >/dev/null 2>&1; then
  JCO="jco"
else
  echo "  ERROR: jco not found after dependency install"
  exit 1
fi

$JCO componentize streaming_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all
SIZE=$(ls -lh "$OUTPUT_WASM" | awk '{print $5}')
echo "  ✓ $OUTPUT_WASM ($SIZE)"
echo ""
echo "Run ./test.sh [HTTP_PORTS...] to deploy and test."
