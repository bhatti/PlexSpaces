#!/usr/bin/env bash
# Build Actor-CSP WASM component (Rust)
# Structured concurrency with supervised actors + Linda tuplespace coordination.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"
OUTPUT_WASM="$SCRIPT_DIR/actor_csp.wasm"

cd "$SCRIPT_DIR"

echo "Building Actor-CSP (Rust WASM)..."

if ! command -v wasm-tools &>/dev/null; then
  echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
  exit 1
fi

export CARGO_TARGET_DIR="${REPO_ROOT}/target"
cargo build --target wasm32-wasip1
CORE_WASM="${CARGO_TARGET_DIR}/wasm32-wasip1/debug/actor_csp_wasm.wasm"
if [ ! -f "$CORE_WASM" ]; then
  cargo build --release --target wasm32-wasip1
  CORE_WASM="${CARGO_TARGET_DIR}/wasm32-wasip1/release/actor_csp_wasm.wasm"
fi
if [ ! -f "$CORE_WASM" ]; then
  echo "ERROR: Core WASM not found. Looked for actor_csp_wasm.wasm in target."
  exit 1
fi

wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o actor_csp_embedded.wasm

ADAPTER=""
for candidate in \
  "$(npm root -g 2>/dev/null)/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/migrating_temporal/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/bank_account/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then
    ADAPTER="$candidate"
    break
  fi
done
if [ -z "$ADAPTER" ]; then
  echo "ERROR: WASI adapter not found. Install jco or run a TypeScript example build first."
  exit 1
fi

wasm-tools component new actor_csp_embedded.wasm --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f actor_csp_embedded.wasm

echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test (start server first: ./scripts/server.sh)."
