#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/examples/python/abstractions"
OUTPUT_WASM="$TARGET_DIR/abstractions_actor.wasm"

mkdir -p "$TARGET_DIR"
cd "$SCRIPT_DIR"

source "$HOME/venv/bin/activate" 2>/dev/null || true
rm -rf wit_world componentize_py_types.py componentize_py_runtime.pyi poll_loop.py componentize_py_async_support 2>/dev/null || true

PYTHON_BIN="$HOME/venv/bin/python3"

run_python() {
  local is_apple_silicon="false"
  if [ "$(uname -s)" = "Darwin" ] && command -v sysctl >/dev/null 2>&1; then
    if [ "$(sysctl -in hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
      is_apple_silicon="true"
    fi
  fi

  if [ "$is_apple_silicon" = "true" ] && command -v arch >/dev/null 2>&1; then
    arch -arm64 "$PYTHON_BIN" "$@"
  else
    "$PYTHON_BIN" "$@"
  fi
}

if ! run_python -c "import plexspaces" 2>/dev/null; then
  run_python -m pip install "plexspaces~=0.1" --quiet
fi

if ! run_python -c "import componentize_py" 2>/dev/null; then
  run_python -m pip install "componentize-py>=0.12" --quiet
fi

run_python -m plexspaces_cli.build build "abstractions_app.py" -o "$OUTPUT_WASM" --wit-dir "$REPO_ROOT/wit/plexspaces-actor"
echo "Built Python abstractions WASM component at $OUTPUT_WASM"
