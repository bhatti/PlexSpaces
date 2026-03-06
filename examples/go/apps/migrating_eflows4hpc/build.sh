#!/usr/bin/env bash
# Build EFlows4HPC Ensemble WASM component (Go/TinyGo)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-simple-actor"
OUTPUT_WASM="$SCRIPT_DIR/ensemble_actor.wasm"

cd "$SCRIPT_DIR"

echo "Building EFlows4HPC Ensemble (Go)..."

if ! command -v tinygo &>/dev/null; then
  echo "ERROR: tinygo not found. Install: brew tap tinygo-org/tools && brew install tinygo"
  exit 1
fi
if ! command -v wasm-tools &>/dev/null; then
  echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
  exit 1
fi

ADAPTER=""
for candidate in \
  "$(npm root -g 2>/dev/null)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/migrating_temporal/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then
    ADAPTER="$candidate"
    break
  fi
done
if [ -z "$ADAPTER" ]; then
  echo "ERROR: WASI adapter not found. Install jco or run a TypeScript example build first."
  exit 1
fi

tinygo build -target=wasi -o ensemble_actor_core.wasm .
wasm-tools component embed "$WIT_DIR" -w actor-world ensemble_actor_core.wasm -o ensemble_actor_embedded.wasm
wasm-tools component new ensemble_actor_embedded.wasm --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f ensemble_actor_core.wasm ensemble_actor_embedded.wasm

echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
