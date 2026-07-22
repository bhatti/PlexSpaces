#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build Alarms API WASM actor using PlexSpaces Python SDK
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
ACTOR_NAME="alarm_queue"

# Activate virtual environment if available
source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"

echo "Building $ACTOR_NAME using PlexSpaces Python SDK..."

# Install SDK if not already installed
if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces SDK..."
    pip install -e "../../../../sdks/python" --quiet --no-build-isolation
fi

# Build using SDK CLI
plexspaces-py build "$ACTOR_NAME.py" -o "$SCRIPT_DIR/${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm | awk '{print $5}'))"
