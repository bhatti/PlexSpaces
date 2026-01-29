#!/bin/bash
# Build Feature Flags Actor for PlexSpaces
#
# Prerequisites:
#   python3.12 -m venv ~/venv
#   source ~/venv/bin/activate
#   pip install componentize-py

set -e
cd "$(dirname "$0")"

# Activate venv if exists
if [ -f ~/venv/bin/activate ]; then
    source ~/venv/bin/activate
fi

ACTOR_NAME="feature_flags_actor"
WIT_DIR="../../../../wit/plexspaces-simple-actor"

echo "Building $ACTOR_NAME..."

componentize-py \
    -d "$WIT_DIR" \
    -w actor-world \
    componentize \
    "$ACTOR_NAME" \
    -o "${ACTOR_NAME}.wasm"

echo "Component built successfully"
ls -lh "${ACTOR_NAME}.wasm" | awk '{print "✅ Built:", $9, "("$5")"}'
