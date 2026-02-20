#!/bin/bash
# Build Rate Limiter WASM actor using TinyGo
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="rate_limiter"

cd "$SCRIPT_DIR"

echo "Building $ACTOR_NAME using TinyGo..."

# Check for tinygo
if ! command -v tinygo &>/dev/null; then
    echo "ERROR: tinygo not found. Install from https://tinygo.org/getting-started/install/"
    echo "  brew install tinygo  (macOS)"
    echo "  snap install tinygo  (Linux)"
    exit 1
fi

# Build WASM module with TinyGo
tinygo build -target=wasi -o "${ACTOR_NAME}.wasm" .

echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
