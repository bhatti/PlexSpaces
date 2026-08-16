#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Build Python WASM perf actor for load testing.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
ACTOR_NAME="perf_actor"

source "$HOME/venv/bin/activate" 2>/dev/null || true

cd "$SCRIPT_DIR"

rm -rf wit_world actor_world componentize_py_types.py componentize_py_runtime.pyi 2>/dev/null || true

if ! python3 -c "import plexspaces" 2>/dev/null; then
    pip install -e "$PROJECT_ROOT/sdks/python" --quiet --no-build-isolation
fi

plexspaces-py build "${ACTOR_NAME}.py" -o "${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "Built: ${ACTOR_NAME}.wasm ($(ls -lh "${ACTOR_NAME}.wasm" | awk '{print $5}'))"
