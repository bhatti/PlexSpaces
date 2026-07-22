#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/web_crawl"
OUTPUT_WASM="$TARGET_DIR/web_crawl_actor.wasm"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

if [ ! -f "node_modules/.bin/tsc" ]; then
  # Install SDK from npm registry (published package — no file: dep needed)
  npm install --no-audit --no-fund
fi

# Rebuild SDK if running from local repo
if [ -d "$REPO_ROOT/sdks/typescript" ] && [ -f "$REPO_ROOT/sdks/typescript/package.json" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
  if [ -d "$REPO_ROOT/sdks/typescript/dist" ] && [ -d "node_modules/@plexspaces/sdk" ]; then
    cp -r "$REPO_ROOT/sdks/typescript/dist/." "node_modules/@plexspaces/sdk/dist/" 2>/dev/null || true
  fi
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

$JCO componentize web_crawl_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all
echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
