#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Build N-Body Python WASM actor using componentize-py

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
# Use simple-actor WIT for Python compatibility (avoids componentize-py lifting issues)
WIT_DIR="$PROJECT_ROOT/wit/plexspaces-simple-actor"
ACTOR_NAME="nbody_actor"
OUTPUT_FILE="$SCRIPT_DIR/${ACTOR_NAME}.wasm"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}🐍 Building N-Body Python WASM Actor${NC}"
echo "========================================"
echo ""
echo "Actor: $SCRIPT_DIR/${ACTOR_NAME}.py"
echo "WIT:   $WIT_DIR"
echo "Output: $OUTPUT_FILE"
echo ""

# Activate virtualenv if it exists (for componentize-py)
if [ -f "$HOME/venv/bin/activate" ]; then
    source "$HOME/venv/bin/activate"
    echo "Using virtualenv: ~/venv"
fi

# Check if componentize-py is installed
if ! command -v componentize-py &> /dev/null; then
    echo -e "${RED}❌ componentize-py not found.${NC}"
    echo ""
    echo "To install:"
    echo "  pip install componentize-py"
    echo ""
    echo "Or activate your virtualenv first:"
    echo "  source ~/venv/bin/activate"
    echo ""
    exit 1
fi

echo "componentize-py version: $(componentize-py --version)"
echo ""

# Build the WASM actor
# Note: componentize-py expects to run from the directory containing the Python module
cd "$SCRIPT_DIR"

# Clean up old bindings first (they may be from the old full WIT)
rm -rf "$SCRIPT_DIR/wit_world" "$SCRIPT_DIR/componentize_py_types.py" "$SCRIPT_DIR/componentize_py_runtime.pyi" "$SCRIPT_DIR/poll_loop.py" "$SCRIPT_DIR/componentize_py_async_support"

echo "Generating bindings from simple-actor WIT..."
componentize-py -d "$WIT_DIR" -w "actor-world" bindings . 2>&1 || true

echo "Building with componentize-py..."
if componentize-py -d "$WIT_DIR" -w "actor-world" componentize -o "$OUTPUT_FILE" "$ACTOR_NAME" 2>&1; then
    echo ""
    echo -e "${GREEN}✅ Build successful!${NC}"
    echo ""
    
    # Show file info
    SIZE=$(ls -lh "$OUTPUT_FILE" | awk '{print $5}')
    echo "WASM file: $OUTPUT_FILE"
    echo "Size: $SIZE (Python WASM includes bundled Python runtime)"
    echo ""
    
    # Optimize with wasm-opt if available
    if command -v wasm-opt &> /dev/null; then
        echo "Optimizing with wasm-opt..."
        ORIG_SIZE=$(stat -f%z "$OUTPUT_FILE" 2>/dev/null || stat -c%s "$OUTPUT_FILE")
        wasm-opt -Oz --strip-debug "$OUTPUT_FILE" -o "${OUTPUT_FILE}.opt" 2>/dev/null || true
        if [ -f "${OUTPUT_FILE}.opt" ] && [ -s "${OUTPUT_FILE}.opt" ]; then
            OPT_SIZE=$(stat -f%z "${OUTPUT_FILE}.opt" 2>/dev/null || stat -c%s "${OUTPUT_FILE}.opt")
            if [ "$OPT_SIZE" -lt "$ORIG_SIZE" ]; then
                mv "${OUTPUT_FILE}.opt" "$OUTPUT_FILE"
                REDUCTION=$(( (ORIG_SIZE - OPT_SIZE) * 100 / ORIG_SIZE ))
                echo -e "${GREEN}✓ Optimized: $(( ORIG_SIZE / 1024 / 1024 ))MB → $(( OPT_SIZE / 1024 / 1024 ))MB (${REDUCTION}% reduction)${NC}"
            else
                rm -f "${OUTPUT_FILE}.opt"
            fi
        fi
    fi
    
    echo ""
    echo "Next steps:"
    echo "  1. Start PlexSpaces node (in Terminal 1):"
    echo "     cargo run -p plexspaces-cli -- start --node-id nbody-node --listen-addr 0.0.0.0:8090"
    echo ""
    echo "  2. Deploy and test (in Terminal 2):"
    echo "     ./test.sh"
else
    echo ""
    echo -e "${RED}❌ Build failed${NC}"
    echo ""
    echo "Common issues:"
    echo "  - Python module doesn't have Actor class"
    echo "  - WIT interface mismatch"
    echo "  - Missing Python dependencies"
    exit 1
fi
