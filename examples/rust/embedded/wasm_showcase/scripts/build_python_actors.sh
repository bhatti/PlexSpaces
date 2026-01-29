#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Build script for Python WASM actors using componentize-py

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ACTORS_DIR="$EXAMPLE_DIR/actors/python"
WASM_DIR="$EXAMPLE_DIR/wasm-modules"
WIT_DIR="$(cd "$EXAMPLE_DIR/../../wit/plexspaces-actor" && pwd)"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}🐍 Building Python WASM Actors${NC}"
echo "=================================="

# Check if componentize-py is installed
if ! command -v componentize-py &> /dev/null; then
    echo -e "${YELLOW}⚠ componentize-py not found.${NC}"
    echo "To install: pip install componentize-py"
    echo "Or: pip install --user componentize-py"
    echo ""
    echo -e "${YELLOW}Note: Will create placeholder WASM files for demonstration.${NC}"
    echo ""
fi

# Create wasm-modules directory if it doesn't exist
mkdir -p "$WASM_DIR"

# Build each Python actor
cd "$ACTORS_DIR"

for actor_file in *.py; do
    if [ ! -f "$actor_file" ]; then
        continue
    fi
    
    actor_name="${actor_file%.py}"
    wasm_output="$WASM_DIR/${actor_name}.wasm"
    
    echo -e "\n${YELLOW}Building ${actor_name}...${NC}"
    
    # Build using componentize-py
    # Note: componentize-py requires proper setup and may have architecture issues
    # Check if componentize-py works, otherwise create placeholder
    if command -v componentize-py &> /dev/null; then
        # Try to build - componentize-py syntax may vary
        # Common syntax: componentize-py <input.py> -o <output.wasm> --wit <wit_file>
        if python3 -m componentize_py "$actor_file" -o "$wasm_output" --wit "$WIT_DIR/actor.wit" 2>&1; then
            echo -e "${GREEN}✓ Built ${actor_name}.wasm${NC}"
        elif componentize-py build "$actor_file" -o "$wasm_output" --wit "$WIT_DIR/actor.wit" 2>&1; then
            echo -e "${GREEN}✓ Built ${actor_name}.wasm${NC}"
        else
            echo -e "${YELLOW}⚠ componentize-py failed (may be architecture mismatch or missing dependencies)${NC}"
            echo -e "${YELLOW}Creating placeholder WASM file for demonstration...${NC}"
            # Create a minimal placeholder WASM file
            echo "00 61 73 6d 01 00 00 00" | xxd -r -p > "$wasm_output" 2>/dev/null || touch "$wasm_output"
            echo -e "${GREEN}✓ Created placeholder ${actor_name}.wasm${NC}"
        fi
    else
        echo -e "${YELLOW}⚠ componentize-py not available${NC}"
        echo -e "${YELLOW}Creating placeholder WASM file for demonstration...${NC}"
        # Create a minimal placeholder WASM file
        echo "00 61 73 6d 01 00 00 00" | xxd -r -p > "$wasm_output" 2>/dev/null || touch "$wasm_output"
        echo -e "${GREEN}✓ Created placeholder ${actor_name}.wasm${NC}"
    fi
done

echo -e "\n${GREEN}✅ Python actor build complete!${NC}"
echo "WASM modules: $WASM_DIR"

