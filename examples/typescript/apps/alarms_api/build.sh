#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build Alarms API WASM component using TypeScript + jco componentize.
#
# Pipeline: TypeScript → ESM bundle → WASM Component
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="alarm_queue_actor"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"

cd "$SCRIPT_DIR"

echo "Building $ACTOR_NAME using TypeScript..."

# Install dependencies if needed
if [ ! -d "node_modules" ]; then
    echo "  Installing dependencies..."
    npm install --silent
fi

# Sync latest SDK dist into node_modules
if [ -d "$REPO_ROOT/sdks/typescript" ]; then
  (cd "$REPO_ROOT/sdks/typescript" && npm run build 2>/dev/null) || true
  if [ -d "$REPO_ROOT/sdks/typescript/dist" ] && [ -d "node_modules/@plexspaces/sdk" ]; then
    cp -r "$REPO_ROOT/sdks/typescript/dist/." "node_modules/@plexspaces/sdk/dist/" 2>/dev/null || true
  fi
fi

# Check for wasm-tools
if ! command -v wasm-tools &>/dev/null; then
    echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
    exit 1
fi

# Step 1: TypeScript → JavaScript
echo "  Step 1: TypeScript → JavaScript"
npm run build:ts

# Step 2: Bundle into single ESM file
echo "  Step 2: Bundle ESM"
npm run build:bundle

# Step 3: Componentize with jco
echo "  Step 3: jco componentize → WASM Component"
node_modules/.bin/jco componentize "${ACTOR_NAME}_bundle.mjs" \
    --wit "$WIT_DIR" \
    --world-name actor-world \
    --out "${ACTOR_NAME}.wasm" \
    --disable http

echo ""
echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
