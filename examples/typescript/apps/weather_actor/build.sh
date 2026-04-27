#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/weather_actor"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$TARGET_DIR/weather_actor.wasm"
ESBUILD_STAMP="$SCRIPT_DIR/node_modules/.esbuild-platform"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

echo "Building Weather Actor (TypeScript WASM)..."

CURRENT_NODE_PLATFORM="$(node -p '`${process.platform}-${process.arch}`')"
INSTALLED_NODE_PLATFORM=""
if [ -f "$ESBUILD_STAMP" ]; then
  INSTALLED_NODE_PLATFORM="$(cat "$ESBUILD_STAMP" 2>/dev/null || true)"
fi

if [ ! -f "node_modules/.bin/tsc" ] || [ ! -f "node_modules/.bin/jco" ] || { [ -n "$INSTALLED_NODE_PLATFORM" ] && [ "$INSTALLED_NODE_PLATFORM" != "$CURRENT_NODE_PLATFORM" ]; }; then
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
elif [ ! -f "$ESBUILD_STAMP" ]; then
  printf '%s\n' "$CURRENT_NODE_PLATFORM" > "$ESBUILD_STAMP"
fi

if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
fi

./node_modules/.bin/tsc -p .
node build-bundle.mjs
./node_modules/.bin/jco componentize weather_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all

SIZE=$(ls -lh "$OUTPUT_WASM" | awk '{print $5}')
echo "  ✓ $OUTPUT_WASM ($SIZE)"
echo ""
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
