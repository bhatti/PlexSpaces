#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build guild_chat WASM component using Rust SDK + WIT
#
# Pipeline: cargo build → core WASM → embed WIT → WASM Component
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="guild_chat"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
BUILD_PROFILE="${CARGO_PROFILE:-debug}"
PROFILE_FLAG=""
if [ "$BUILD_PROFILE" = "release" ]; then
  PROFILE_FLAG="--release"
fi
CORE_WASM="$REPO_ROOT/target/wasm32-wasip1/$BUILD_PROFILE/guild_chat_wasm.wasm"
OUTPUT_WASM="$SCRIPT_DIR/${ACTOR_NAME}.wasm"
EMBEDDED_WASM="$SCRIPT_DIR/${ACTOR_NAME}_embedded.wasm"

cd "$SCRIPT_DIR"
export CARGO_TARGET_DIR="$REPO_ROOT/target"

if ! command -v wasm-tools >/dev/null 2>&1; then
  echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
  exit 1
fi

echo "Building $ACTOR_NAME (Rust WASM, SDK+WIT)..."
cargo build $PROFILE_FLAG --target wasm32-wasip1 ${CARGO_BUILD_FLAGS:-}

if [ ! -f "$CORE_WASM" ]; then
  echo "ERROR: core WASM not found at $CORE_WASM"
  exit 1
fi

echo "  Embedding WIT metadata..."
wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o "$EMBEDDED_WASM"

# Find WASI adapter (reactor mode for library/actor modules)
ADAPTER=""
GLOBAL_NPM="$(npm root -g 2>/dev/null || echo /usr/local/lib/node_modules)"
for candidate in \
    "$GLOBAL_NPM/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
    "$REPO_ROOT/examples/typescript/apps/bank_account/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
    "$REPO_ROOT/examples/typescript/apps/migrating_cloudflare_workers/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
    if [ -f "$candidate" ]; then
        ADAPTER="$candidate"
        break
    fi
done
if [ -z "$ADAPTER" ]; then
    echo "ERROR: WASI adapter not found."
    echo "  Install jco globally: npm install -g @bytecodealliance/jco"
    echo "  Or locally: cd $REPO_ROOT/examples/typescript/apps/bank_account && npm install"
    exit 1
fi

echo "  Creating WASM Component..."
wasm-tools component new "$EMBEDDED_WASM" \
    --adapt "wasi_snapshot_preview1=$ADAPTER" \
    -o "$OUTPUT_WASM"

rm -f "$EMBEDDED_WASM"
echo ""
echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
