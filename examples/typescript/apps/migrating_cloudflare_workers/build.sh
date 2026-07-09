#!/usr/bin/env bash
# Build Guild Chat WASM actor: TypeScript → JS → ESM bundle → WASM component
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/guild_chat_actor.wasm"

cd "$SCRIPT_DIR"

echo "Building TypeScript guild_chat → WASM..."
echo ""

# 1. Ensure deps (@plexspaces/sdk, typescript, jco, esbuild)
if [ ! -f "node_modules/.bin/tsc" ]; then
  echo "  Installing dependencies..."
  npm install --no-audit --no-fund
fi

# 2. Build SDK (needed for tsc and bundle)
if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
  if [ -d "$REPO_ROOT/sdks/typescript/dist" ] && [ -d "node_modules/@plexspaces/sdk" ]; then
    cp -r "$REPO_ROOT/sdks/typescript/dist/." "node_modules/@plexspaces/sdk/dist/"
  fi
fi

# 3. Compile TypeScript → JS (CJS for Node verify)
./node_modules/.bin/tsc -p .
if [ ! -f "guild_chat_actor.js" ]; then
  echo "  ERROR: guild_chat_actor.js not produced"
  exit 1
fi
echo "  ✓ guild_chat_actor.js"

# 4. Bundle actor + SDK into one ESM file for jco (no Node APIs in bundle)
if command -v node &>/dev/null; then
  node build-bundle.mjs
else
  echo "  ERROR: node not found"
  exit 1
fi

# 5. Build WASM component via jco componentize (same WIT world as Python/Go)
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

# jco componentize the bundle (exports actor; no WASI so only plexspaces:actor/host).
if ! $JCO componentize guild_chat_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all 2>&1; then
  echo "  ERROR: jco componentize failed (see above)."
  exit 1
fi

SIZE=$(ls -lh "$OUTPUT_WASM" | awk '{print $5}')
echo "  ✓ $OUTPUT_WASM ($SIZE)"
echo ""
echo "Build complete. Run ./test.sh to deploy and test."
