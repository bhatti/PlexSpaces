#!/bin/bash
# Build GPU Job Scheduler WASM actor (Python) - Kueue style
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../../" && pwd)"
SDK_DIR="$PROJECT_ROOT/sdks/python"
ACTOR_NAME="job_scheduler_actor"

if [ -n "${VIRTUAL_ENV:-}" ]; then
    :
elif [ -f "$HOME/venv/bin/activate" ]; then
    source "$HOME/venv/bin/activate"
fi

cd "$SCRIPT_DIR"
export PYTHONPATH="$SDK_DIR${PYTHONPATH:+:$PYTHONPATH}"
rm -rf wit_world componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support 2>/dev/null || true

echo "Building $ACTOR_NAME (Python GenServer + locks - Kueue-style)..."

if ! python3 -c "import plexspaces" 2>/dev/null; then
    echo "Installing PlexSpaces SDK..."
    pip install -e "$SDK_DIR" --quiet
fi

python3 -m plexspaces_cli.build build "${ACTOR_NAME}.py" -o "${ACTOR_NAME}.wasm" --wit-dir "$PROJECT_ROOT/wit/plexspaces-actor"

echo "  ✓ ${ACTOR_NAME}.wasm ($(ls -lh ${ACTOR_NAME}.wasm 2>/dev/null | awk '{print $5}' || echo 'built'))"
echo "Run ./test.sh [HTTP_PORT] to deploy and test."
