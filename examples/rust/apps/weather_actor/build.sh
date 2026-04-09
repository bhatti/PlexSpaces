#!/usr/bin/env bash
# Build Weather Actor WASM (Rust)
# SPDX-License-Identifier: LGPL-2.1-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/weather_actor.wasm"
EMBEDDED_WASM="$SCRIPT_DIR/weather_actor_embedded.wasm"
BUILD_PROFILE="${CARGO_PROFILE:-debug}"
PROFILE_FLAG=""
[ "$BUILD_PROFILE" = "release" ] && PROFILE_FLAG="--release"
CORE_WASM="$REPO_ROOT/target/wasm32-wasip1/$BUILD_PROFILE/weather_actor.wasm"

cd "$SCRIPT_DIR"
export CARGO_TARGET_DIR="$REPO_ROOT/target"

for cmd in wasm-tools rustup; do
    command -v "$cmd" >/dev/null 2>&1 || { echo "ERROR: $cmd not found"; exit 1; }
done

rustup target list --installed | grep -qx 'wasm32-wasip1' || {
    echo "ERROR: run: rustup target add wasm32-wasip1"; exit 1
}

[ -f "$WIT_DIR/world.wit" ] || { echo "ERROR: WIT not found at $WIT_DIR"; exit 1; }

echo "Building weather_actor (Rust WASM)..."
cargo build $PROFILE_FLAG --target wasm32-wasip1 ${CARGO_BUILD_FLAGS:-}
[ -f "$CORE_WASM" ] || { echo "ERROR: core WASM not found at $CORE_WASM"; exit 1; }

wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o "$EMBEDDED_WASM"

ADAPTER=""
for candidate in \
  "$(npm root -g 2>/dev/null)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/migrating_temporal/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
  [ -f "$candidate" ] && { ADAPTER="$candidate"; break; }
done
[ -n "$ADAPTER" ] || { echo "ERROR: WASI adapter not found"; exit 1; }

wasm-tools component new "$EMBEDDED_WASM" --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f "$EMBEDDED_WASM"
echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
