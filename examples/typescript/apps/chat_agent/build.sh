#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build ChatAgent WASM actor: TypeScript → JS → ESM bundle → WASM component
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/chat_agent_actor.wasm"

cd "$SCRIPT_DIR"

echo "Building TypeScript chat_agent → WASM..."

# 1. Install dependencies
if [ ! -f "node_modules/.bin/tsc" ]; then
  echo "  Installing dependencies..."
  npm install --no-audit --no-fund
fi

# 2. Build SDK
if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
  if [ -d "$REPO_ROOT/sdks/typescript/dist" ] && [ -d "node_modules/@plexspaces/sdk" ]; then
    cp -r "$REPO_ROOT/sdks/typescript/dist/." "node_modules/@plexspaces/sdk/dist/" 2>/dev/null || true
  fi
fi

# 3. Compile TypeScript → JS
./node_modules/.bin/tsc -p .
if [ ! -f "chat_agent_actor.js" ]; then
  echo "  ERROR: chat_agent_actor.js not produced"
  exit 1
fi
echo "  ✓ chat_agent_actor.js"

# 4. Bundle for jco componentize
node build-bundle.mjs

# 5. Build WASM component
if [ ! -d "$WIT_DIR" ] || [ ! -f "$WIT_DIR/world.wit" ]; then
  echo "  ERROR: WIT dir not found: $WIT_DIR"
  exit 1
fi

if [ -f "node_modules/.bin/jco" ]; then
  JCO="./node_modules/.bin/jco"
elif command -v jco &>/dev/null; then
  JCO="jco"
else
  echo "  Installing jco..."
  npm install --no-audit --no-fund @bytecodealliance/jco @bytecodealliance/componentize-js
  JCO="./node_modules/.bin/jco"
fi

if ! $JCO componentize chat_agent_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all 2>&1; then
  echo "  ERROR: jco componentize failed (see above)."
  exit 1
fi

SIZE=$(ls -lh "$OUTPUT_WASM" | awk '{print $5}')
echo "  ✓ $OUTPUT_WASM ($SIZE)"
echo "Run ./test.sh to deploy and test."
