#!/bin/bash
# Build Python WASM Calculator using componentize-py
#
# This script builds a Python actor as a WebAssembly Component using:
# - componentize-py: Compiles Python to WASM with Component Model support
# - WIT interface: plexspaces-simple-actor (string-only interface for compatibility)
#
# Prerequisites:
#   1. Python 3.12+ virtual environment with componentize-py
#      python3.12 -m venv ~/venv
#      source ~/venv/bin/activate
#      pip install componentize-py
#
# Key Design Decisions:
#   - Uses simple-actor WIT interface (strings only) to avoid PyObject_SetItem errors
#   - All complex data passed as JSON strings
#   - No environment variable inheritance (prevents WASI setenv issues)
#
# See examples/python/README.md for full documentation

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WIT_DIR="$PROJECT_ROOT/wit/plexspaces-simple-actor"
ACTOR_NAME="$(basename "$SCRIPT_DIR")_actor"

# Activate Python venv
if [ -f "$HOME/venv/bin/activate" ]; then
    source "$HOME/venv/bin/activate"
else
    echo "⚠️  Python venv not found at ~/venv"
    echo "   Create with: python3.12 -m venv ~/venv && source ~/venv/bin/activate && pip install componentize-py"
    exit 1
fi

cd "$SCRIPT_DIR"

# Clean previous artifacts
rm -rf wit_world wit_world.pyi componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support __pycache__ 2>/dev/null || true

echo "Building $ACTOR_NAME..."

# Step 1: Generate Python bindings from WIT
# This creates wit_world/ directory with Python stubs
componentize-py -d "$WIT_DIR" -w "actor-world" bindings .

# Step 2: Compile Python to WASM Component
# Creates a ~35MB component (includes CPython interpreter)
componentize-py -d "$WIT_DIR" -w "actor-world" componentize -o "${ACTOR_NAME}.wasm" "$ACTOR_NAME"

echo "✅ Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
echo ""
echo "Deploy with:"
echo "  cargo run -p plexspaces-cli -- deploy --node localhost:8090 -i calculator-test -n calculator -w ${SCRIPT_DIR}/${ACTOR_NAME}.wasm"
