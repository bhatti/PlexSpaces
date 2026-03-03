#!/bin/bash
# Build Order Fulfillment Workflow WASM actor (Python)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SDK_DIR="$PROJECT_ROOT/sdks/python"
ACTOR_NAME="order_fulfillment_actor"

source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"
rm -rf wit_world componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support 2>/dev/null || true

echo "Building $ACTOR_NAME (Python workflow_actor)..."

if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces SDK..."
    pip install -e "$SDK_DIR" --quiet
fi

plexspaces-py build "${ACTOR_NAME}.py" -o "${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-simple-actor"

echo "  ✓ ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
