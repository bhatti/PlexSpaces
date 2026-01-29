#!/bin/bash
# Build Bank Account WASM actor
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$PROJECT_ROOT/wit/plexspaces-simple-actor"
ACTOR_NAME="account_actor"

source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"
rm -rf wit_world componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support 2>/dev/null || true

echo "Building $ACTOR_NAME..."
componentize-py -d "$WIT_DIR" -w "actor-world" bindings .
componentize-py -d "$WIT_DIR" -w "actor-world" componentize -o "${ACTOR_NAME}.wasm" "$ACTOR_NAME"

echo "✅ Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
