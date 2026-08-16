#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Builds dispatch_test_actor.wasm — minimal two-class fixture for dispatch tests.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../../.." && pwd)"
SDK_DIR="$REPO_ROOT/sdks/python"
OUTPUT_WASM="$SCRIPT_DIR/dispatch_test_actor.wasm"

source "$HOME/venv/bin/activate" 2>/dev/null || true

PYTHON_BIN="$HOME/venv/bin/python3"

run_python() {
  if [ "$(uname -s)" = "Darwin" ] && [ "$(sysctl -in hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
    arch -arm64 "$PYTHON_BIN" "$@"
  else
    "$PYTHON_BIN" "$@"
  fi
}

if ! run_python -c "import plexspaces" 2>/dev/null; then
  run_python -m pip install -e "$SDK_DIR" --quiet
fi

if ! run_python -c "import componentize_py" 2>/dev/null; then
  run_python -m pip install "componentize-py>=0.12" --quiet
fi

run_python -m plexspaces_cli.build build "dispatch_test_actor.py" -o "$OUTPUT_WASM" --wit-dir "$REPO_ROOT/wit/plexspaces-actor"
echo "Built dispatch_test_actor.wasm at $OUTPUT_WASM"
