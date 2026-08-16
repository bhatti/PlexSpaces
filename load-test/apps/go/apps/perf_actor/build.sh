#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build Go WASM perf actor for load testing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/load-test/go/perf_actor"
ACTOR_NAME="perf_actor"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
CORE_WASM="$TARGET_DIR/${ACTOR_NAME}_core.wasm"
EMBEDDED_WASM="$TARGET_DIR/${ACTOR_NAME}_embedded.wasm"
OUTPUT_WASM="$TARGET_DIR/${ACTOR_NAME}.wasm"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

if ! command -v tinygo >/dev/null 2>&1; then
  echo "ERROR: tinygo not found. Install from https://tinygo.org/getting-started/install/"
  exit 1
fi
if ! command -v wasm-tools >/dev/null 2>&1; then
  echo "ERROR: wasm-tools not found. Install with: cargo install wasm-tools"
  exit 1
fi

export GOTOOLCHAIN=go1.25.5
if ! command -v wasm-opt &>/dev/null; then
  _JCO_WASMOPT="$(npm root -g 2>/dev/null)/@bytecodealliance/jco/node_modules/binaryen/bin/wasm-opt"
  if [ -f "$_JCO_WASMOPT" ]; then
    export WASMOPT="$_JCO_WASMOPT"
  else
    echo "ERROR: wasm-opt not found. Install binaryen: brew install binaryen"
    exit 1
  fi
fi

ADAPTER=""
GLOBAL_NPM="$(npm root -g 2>/dev/null || echo /usr/local/lib/node_modules)"
for candidate in \
  "$GLOBAL_NPM/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/bank_account/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$HOME/.wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then
    ADAPTER="$candidate"
    break
  fi
done

if [ -z "$ADAPTER" ]; then
  echo "ERROR: WASI adapter not found. Run: npm install -g @bytecodealliance/jco"
  exit 1
fi

go mod tidy
tinygo build -target=wasi -o "$CORE_WASM" .
wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o "$EMBEDDED_WASM"
wasm-tools component new "$EMBEDDED_WASM" --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f "$CORE_WASM" "$EMBEDDED_WASM"

echo "Built: $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
