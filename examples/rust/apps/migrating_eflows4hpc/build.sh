#!/usr/bin/env bash
# Build EFlows4HPC Ensemble WASM component (Rust)
# Same pipeline as migrating_temporal: core wasm -> component embed (WIT) -> component new (WASI adapter)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$REPO_ROOT/wit/plexspaces-simple-actor"
OUTPUT_WASM="$SCRIPT_DIR/ensemble_actor.wasm"

cd "$SCRIPT_DIR"

echo "Building EFlows4HPC Ensemble (Rust)..."

if ! command -v wasm-tools &>/dev/null; then
  echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
  exit 1
fi

# Build core wasm (exports match plexspaces-simple-actor ABI). Uses shared target via .cargo/config.toml.
cargo build --release --target wasm32-wasip1
CORE_WASM="$REPO_ROOT/target/wasm32-wasip1/release/migrating_eflows4hpc_wasm.wasm"
if [ ! -f "$CORE_WASM" ]; then
  CORE_WASM="$SCRIPT_DIR/../../../../target/wasm32-wasip1/release/migrating_eflows4hpc_wasm.wasm"
fi
if [ ! -f "$CORE_WASM" ]; then
  echo "ERROR: Core WASM not found. Looked for migrating_eflows4hpc_wasm.wasm in workspace target (wasm32-wasip1/release/)."
  exit 1
fi

# Embed WIT types into the core module (actor-world)
wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o ensemble_actor_embedded.wasm

# Wrap with WASI adapter so the runtime can instantiate
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

wasm-tools component new ensemble_actor_embedded.wasm --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f ensemble_actor_embedded.wasm

echo "  ✓ $OUTPUT_WASM ($(ls -lh "$OUTPUT_WASM" | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
