#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build ChatAgent (Python WASM)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="chat_agent"

source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"

echo "Building ChatAgent (Python WASM)..."

if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces Python SDK..."
    pip install -e "../../../../sdks/python" --quiet --no-build-isolation
fi

plexspaces-py build "${ACTOR_NAME}.py" -o "$SCRIPT_DIR/${ACTOR_NAME}_actor.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "  ✓ $SCRIPT_DIR/${ACTOR_NAME}_actor.wasm ($(ls -lh "${ACTOR_NAME}_actor.wasm" | awk '{print $5}'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
