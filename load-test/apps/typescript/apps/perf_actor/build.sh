#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build TypeScript WASM perf actor for load testing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
TARGET_DIR="$REPO_ROOT/target/load-test/typescript/perf_actor"
OUTPUT_WASM="$TARGET_DIR/perf_actor.wasm"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

if [ ! -f "node_modules/.bin/tsc" ]; then
  npm install --no-audit --no-fund
fi

# Sync latest SDK build
if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
  if [ -d "$REPO_ROOT/sdks/typescript/dist" ] && [ -d "node_modules/@plexspaces/sdk" ]; then
    cp -r "$REPO_ROOT/sdks/typescript/dist/." "node_modules/@plexspaces/sdk/dist/" 2>/dev/null || true
  fi
fi

./node_modules/.bin/tsc -p .
node build-bundle.mjs

if command -v jco >/dev/null 2>&1; then
  JCO="jco"
elif [ -f "node_modules/.bin/jco" ]; then
  JCO="./node_modules/.bin/jco"
else
  npm install --no-audit --no-fund @bytecodealliance/jco @bytecodealliance/componentize-js
  JCO="./node_modules/.bin/jco"
fi

$JCO componentize perf_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all
echo "Built: $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
