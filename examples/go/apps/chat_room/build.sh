#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/examples/go/chat_room"
ACTOR_NAME="chat_room_actor"
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

ADAPTER=""
GLOBAL_NPM="$(npm root -g 2>/dev/null || echo /usr/local/lib/node_modules)"
for candidate in \
  "$GLOBAL_NPM/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/bank_account/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$REPO_ROOT/examples/typescript/apps/migrating_cloudflare_workers/node_modules/@bytecodealliance/jco/lib/wasi_snapshot_preview1.reactor.wasm" \
  "$HOME/.wasi_snapshot_preview1.reactor.wasm"; do
  if [ -f "$candidate" ]; then
    ADAPTER="$candidate"
    break
  fi
done

if [ -z "$ADAPTER" ]; then
  echo "ERROR: WASI adapter not found. Install jco globally with: npm install -g @bytecodealliance/jco"
  exit 1
fi

echo "Building $ACTOR_NAME using TinyGo..."
go mod tidy
tinygo build -target=wasi -o "$CORE_WASM" .
wasm-tools component embed "$WIT_DIR" -w actor-world "$CORE_WASM" -o "$EMBEDDED_WASM"
wasm-tools component new "$EMBEDDED_WASM" --adapt "wasi_snapshot_preview1=$ADAPTER" -o "$OUTPUT_WASM"
rm -f "$CORE_WASM" "$EMBEDDED_WASM"

echo "Built Go chat_room WASM component at $OUTPUT_WASM"
