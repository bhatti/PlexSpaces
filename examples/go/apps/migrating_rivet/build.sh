#!/bin/bash
# Build Game Matchmaking WASM component using TinyGo
#
# Pipeline: TinyGo → core WASM → embed WIT → WASM Component
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="matchmaking"
WIT_DIR="$REPO_ROOT/wit/plexspaces-actor"

cd "$SCRIPT_DIR"

echo "Building $ACTOR_NAME using TinyGo..."

# Check for tinygo
if ! command -v tinygo &>/dev/null; then
    echo "ERROR: tinygo not found. Install from https://tinygo.org/getting-started/install/"
    echo "  brew tap tinygo-org/tools && brew install tinygo  (macOS)"
    echo "  snap install tinygo  (Linux)"
    exit 1
fi

# Check for wasm-tools
if ! command -v wasm-tools &>/dev/null; then
    echo "ERROR: wasm-tools not found. Install: cargo install wasm-tools"
    exit 1
fi

# Support Go 1.26+ with TinyGo (TinyGo checks GOTOOLCHAIN for compatibility)
export GOTOOLCHAIN=go1.25.5
# Locate wasm-opt (required by TinyGo); fall back to the copy bundled with jco
if ! command -v wasm-opt &>/dev/null; then
  _JCO_WASMOPT="$(npm root -g 2>/dev/null)/@bytecodealliance/jco/node_modules/binaryen/bin/wasm-opt"
  if [ -f "$_JCO_WASMOPT" ]; then
    export WASMOPT="$_JCO_WASMOPT"
  else
    echo "ERROR: wasm-opt not found. Install binaryen: brew install binaryen"
    exit 1
  fi
fi

# Find WASI adapter (reactor mode for library/actor modules)
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
    echo "ERROR: WASI adapter not found."
    echo "  Install jco globally: npm install -g @bytecodealliance/jco"
    echo "  Or locally in a TS example: cd $REPO_ROOT/examples/typescript/apps/bank_account && npm install"
    exit 1
fi

# Step 1: Build core WASM module with TinyGo
echo "  Step 1: TinyGo → core WASM module"
go mod tidy
tinygo build -target=wasi -o "${ACTOR_NAME}_core.wasm" .
echo "    core module: $(ls -lh ${ACTOR_NAME}_core.wasm | awk '{print $5}')"

# Step 2: Embed WIT metadata into the core module
echo "  Step 2: Embed WIT metadata"
wasm-tools component embed "$WIT_DIR" -w actor-world \
    "${ACTOR_NAME}_core.wasm" -o "${ACTOR_NAME}_embedded.wasm"

# Step 3: Convert to WASM Component with WASI adapter
echo "  Step 3: Create WASM Component"
wasm-tools component new "${ACTOR_NAME}_embedded.wasm" \
    --adapt "wasi_snapshot_preview1=$ADAPTER" \
    -o "${ACTOR_NAME}.wasm"

# Cleanup intermediate files
rm -f "${ACTOR_NAME}_core.wasm" "${ACTOR_NAME}_embedded.wasm"

echo ""
echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
