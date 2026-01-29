#!/bin/bash
# Build Receipt Storage Actor for PlexSpaces
#
# This script builds a Python WASM component using componentize-py.
# The resulting .wasm file can be deployed to any PlexSpaces node.
#
# Prerequisites:
#   python3.12 -m venv ~/venv
#   source ~/venv/bin/activate
#   pip install componentize-py
#
# Key insight: componentize-py bundles the Python runtime into the WASM.
# The PyObject_SetItem error was fixed in the WASI runtime by not inheriting
# environment variables (see crates/wasm-runtime/src/instance.rs).

set -e
cd "$(dirname "$0")"

# Activate venv if exists
if [ -f ~/venv/bin/activate ]; then
    source ~/venv/bin/activate
fi

ACTOR_NAME="receipt_actor"
WIT_DIR="../../../../wit/plexspaces-simple-actor"

echo "Building $ACTOR_NAME..."

# Build WASM component with componentize-py
componentize-py \
    -d "$WIT_DIR" \
    -w actor-world \
    componentize \
    "$ACTOR_NAME" \
    -o "${ACTOR_NAME}.wasm"

echo "Component built successfully"
ls -lh "${ACTOR_NAME}.wasm" | awk '{print "✅ Built:", $9, "("$5")"}'
