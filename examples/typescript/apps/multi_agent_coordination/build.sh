#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/multi_agent_coordination"
OUTPUT_WASM="$TARGET_DIR/multi_agent_coordination_actor.wasm"
ACTOR_NAME="multi_agent_coordination_actor"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

if [ ! -f "node_modules/.bin/tsc" ]; then
  npm install --no-audit --no-fund
fi

if [ -d "$REPO_ROOT/sdks/typescript" ]; then
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

$JCO componentize "${ACTOR_NAME}_bundle.mjs" --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all
echo "Built TypeScript multi-agent coordination WASM component at $OUTPUT_WASM"
