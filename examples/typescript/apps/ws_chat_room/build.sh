#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/ws_chat_room"
OUTPUT_WASM="$TARGET_DIR/chat_server_actor.wasm"

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

# Build WASM actor bundle
./node_modules/.bin/tsc -p .
node build-bundle.mjs

# Build browser client bundle
node build-client.mjs

# Componentize to WASM
if command -v jco >/dev/null 2>&1; then
  JCO="jco"
elif [ -f "node_modules/.bin/jco" ]; then
  JCO="./node_modules/.bin/jco"
else
  npm install --no-audit --no-fund @bytecodealliance/jco @bytecodealliance/componentize-js
  JCO="./node_modules/.bin/jco"
fi

$JCO componentize chat_server_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all
echo "Built ws_chat_room WASM at $OUTPUT_WASM"
echo ""
echo "Deploy the app zip (includes static UI) with:"
echo "  bash test.sh   # builds zip, deploys, runs integration tests"
echo "  Then open: http://localhost:8091/apps/ts-ws-chat-room/"
echo ""
echo "Run automated test: bash test.sh"
