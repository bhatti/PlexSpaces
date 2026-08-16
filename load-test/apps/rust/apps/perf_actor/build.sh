#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build Rust WASM perf actor for load testing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
TARGET_DIR="$REPO_ROOT/target/load-test/rust/perf_actor"
CRATE_NAME="perf_actor_wasm"
CORE_WASM="$REPO_ROOT/target/wasm32-wasip1/release/${CRATE_NAME}.wasm"
EMBEDDED_WASM="$TARGET_DIR/${CRATE_NAME}_embedded.wasm"
OUTPUT_WASM="$TARGET_DIR/perf_actor.wasm"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

if ! command -v wasm-tools >/dev/null 2>&1; then
  echo "ERROR: wasm-tools not found. Install with: cargo install wasm-tools"
  exit 1
fi

if ! rustup target list --installed 2>/dev/null | grep -qx 'wasm32-wasip1'; then
  echo "ERROR: Rust target wasm32-wasip1 not installed. Run: rustup target add wasm32-wasip1"
  exit 1
fi

CARGO_TARGET_DIR="$REPO_ROOT/target" cargo build --release --target wasm32-wasip1

wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o "$EMBEDDED_WASM"

# WASI preview1 adapter — required to componentize binaries that use wasi_snapshot_preview1
ADAPTER=""
for candidate in \
  "$(npm root -g 2>/dev/null)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/bank_account/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/load-test/apps/typescript/apps/perf_actor/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then
    ADAPTER="$candidate"
    break
  fi
done

if [ -z "$ADAPTER" ]; then
  echo "ERROR: WASI preview1 adapter not found."
  echo "  Fix: npm install -g @bytecodealliance/jco  OR  run: bash load-test/build.sh typescript first"
  exit 1
fi

wasm-tools component new "$EMBEDDED_WASM" \
  --adapt "wasi_snapshot_preview1=$ADAPTER" \
  -o "$OUTPUT_WASM"
rm -f "$EMBEDDED_WASM"

echo "Built: $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
