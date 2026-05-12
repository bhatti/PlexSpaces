#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/chat_room_actor.wasm"
EMBEDDED_WASM="$SCRIPT_DIR/chat_room_actor_embedded.wasm"
BUILD_PROFILE="${CARGO_PROFILE:-debug}"
PROFILE_FLAG=""
if [ "$BUILD_PROFILE" = "release" ]; then
  PROFILE_FLAG="--release"
fi
CORE_WASM="$REPO_ROOT/target/wasm32-wasip1/$BUILD_PROFILE/chat_room_wasm.wasm"

cd "$SCRIPT_DIR"
export CARGO_TARGET_DIR="$REPO_ROOT/target"

if ! command -v wasm-tools >/dev/null 2>&1; then
  echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
  exit 1
fi

echo "Building chat_room (Rust WASM)..."
cargo build $PROFILE_FLAG --target wasm32-wasip1 ${CARGO_BUILD_FLAGS:-}

if [ ! -f "$CORE_WASM" ]; then
  echo "ERROR: core WASM not found at $CORE_WASM"
  exit 1
fi

wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o "$EMBEDDED_WASM"

ADAPTER=""
for candidate in \
  "$(npm root -g 2>/dev/null)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/migrating_temporal/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/bank_account/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/chat_room/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then
    ADAPTER="$candidate"
    break
  fi
done
if [ -z "$ADAPTER" ]; then
  echo "ERROR: WASI adapter not found. Install: npm install -g @bytecodealliance/jco"
  exit 1
fi

wasm-tools component new "$EMBEDDED_WASM" \
  --adapt "wasi_snapshot_preview1=$ADAPTER" \
  -o "$OUTPUT_WASM"
rm -f "$EMBEDDED_WASM"

echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
