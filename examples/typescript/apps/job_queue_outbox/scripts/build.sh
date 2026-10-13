#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build TypeScript job_queue_outbox to WASM component.
# Uses: tsc -> JS, esbuild bundle -> ESM, jco componentize -> WASM.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$EXAMPLE_DIR/../../../../" && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$EXAMPLE_DIR/job_queue_outbox_actor.wasm"

cd "$EXAMPLE_DIR"

echo "Building TypeScript job_queue_outbox -> WASM..."
echo ""

# 1. Ensure deps
if [ ! -f "node_modules/.bin/tsc" ]; then
  echo "  Installing dependencies..."
  npm install --no-audit --no-fund
fi

# 2. Build SDK
if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
fi

# 3. Compile TypeScript -> JS
./node_modules/.bin/tsc -p .
if [ ! -f "job_queue_outbox_actor.js" ]; then
  echo "  ERROR: job_queue_outbox_actor.js not produced"
  exit 1
fi
echo "  ✓ job_queue_outbox_actor.js"

# 4. Bundle actor + SDK into one ESM file
if command -v node &>/dev/null; then
  node build-bundle.mjs
else
  echo "  ERROR: node not found"
  exit 1
fi

# 5. Build WASM component via jco componentize
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

if ! $JCO componentize job_queue_outbox_actor_bundle.mjs --wit "$WIT_DIR" -n actor-world -o "$OUTPUT_WASM" --disable all 2>&1; then
  echo "  ERROR: jco componentize failed (see above)."
  exit 1
fi
echo "  ✓ $OUTPUT_WASM"
echo ""
echo "Build complete."
