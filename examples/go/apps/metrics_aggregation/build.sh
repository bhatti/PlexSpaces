#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/metrics_aggregation_actor.wasm"
export GOCACHE="${GOCACHE:-$REPO_ROOT/.cache/go-build}"
mkdir -p "$GOCACHE"
BUILD_HOME="${PLEXSPACES_BUILD_HOME:-$REPO_ROOT/.cache/tinygo-home}"
mkdir -p "$BUILD_HOME"

cd "$SCRIPT_DIR"

echo "Building Metrics Aggregation (Go WASM)..."

if ! command -v tinygo &>/dev/null; then
  echo "ERROR: tinygo not found."; exit 1
fi
if ! command -v wasm-tools &>/dev/null; then
  echo "ERROR: wasm-tools not found."; exit 1
fi

export GOTOOLCHAIN=go1.25.5
if ! command -v wasm-opt &>/dev/null; then
  _JCO_WASMOPT="$(npm root -g 2>/dev/null)/@bytecodealliance/jco/node_modules/binaryen/bin/wasm-opt"
  if [ -f "$_JCO_WASMOPT" ]; then export WASMOPT="$_JCO_WASMOPT"
  else echo "ERROR: wasm-opt not found."; exit 1; fi
fi

ADAPTER=""
for candidate in \
  "$(npm root -g 2>/dev/null)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/streaming_pipeline/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then ADAPTER="$candidate"; break; fi
done
if [ -z "$ADAPTER" ]; then echo "ERROR: WASI adapter not found."; exit 1; fi

HOME="$BUILD_HOME" tinygo build -target=wasi -o metrics_aggregation_actor_core.wasm .
wasm-tools component embed "$WIT_DIR" -w actor-world metrics_aggregation_actor_core.wasm -o metrics_aggregation_actor_embedded.wasm
wasm-tools component new metrics_aggregation_actor_embedded.wasm --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f metrics_aggregation_actor_core.wasm metrics_aggregation_actor_embedded.wasm

echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
