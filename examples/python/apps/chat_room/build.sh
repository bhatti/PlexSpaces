#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build Chat Room WASM actor using PlexSpaces Python SDK
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="chat_room_actor"

# Activate virtual environment if available
source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"

# Clean up old boilerplate files (no longer needed with SDK)
rm -rf wit_world componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support 2>/dev/null || true

echo "Building $ACTOR_NAME using PlexSpaces SDK..."

# Install SDK if not already installed
if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces SDK..."
    pip install "plexspaces~=0.1" --quiet
fi

# Build using SDK CLI (use absolute output path)
plexspaces-py build "$ACTOR_NAME.py" -o "$SCRIPT_DIR/${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
