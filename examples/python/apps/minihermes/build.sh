#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build MiniHermes Python WASM actor using PlexSpaces Python SDK.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="minihermes_actor"

source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"

echo "Building MiniHermes (Python WASM)..."

if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces Python SDK..."
    pip install -e "../../../../sdks/python" --quiet --no-build-isolation
fi

plexspaces-py build "$ACTOR_NAME.py" -o "$SCRIPT_DIR/${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "  ✓ $SCRIPT_DIR/${ACTOR_NAME}.wasm ($(ls -lh "${ACTOR_NAME}.wasm" | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
