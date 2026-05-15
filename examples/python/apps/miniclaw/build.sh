#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build MiniClaw Python WASM actor using PlexSpaces Python SDK
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="miniclaw_actor"

source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"

rm -rf wit_world componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support 2>/dev/null || true

echo "Building MiniClaw (Python WASM)..."

if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces SDK..."
    pip install "plexspaces~=0.1" --quiet
fi

plexspaces-py build "$ACTOR_NAME.py" -o "$SCRIPT_DIR/${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "  ✓ $SCRIPT_DIR/${ACTOR_NAME}.wasm ($(ls -lh "${ACTOR_NAME}.wasm" | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
