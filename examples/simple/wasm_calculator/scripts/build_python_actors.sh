#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Build script for Python WASM calculator actors using componentize-py

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ACTORS_DIR="$EXAMPLE_DIR/actors/python"
WASM_DIR="$EXAMPLE_DIR/wasm-modules"
WIT_DIR="$(cd "$EXAMPLE_DIR/../../../wit/plexspaces-actor" && pwd)"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${GREEN}🐍 Building Python WASM Calculator Actors${NC}"
echo "=============================================="

# Check if componentize-py is installed and functional
COMPONENTIZE_PY_AVAILABLE=false
COMPONENTIZE_PY_WORKING=false

if command -v componentize-py &> /dev/null; then
    COMPONENTIZE_PY_AVAILABLE=true
    # Test if componentize-py actually works (not just installed)
    if componentize-py --version &>/dev/null || componentize-py --help &>/dev/null; then
        COMPONENTIZE_PY_WORKING=true
    else
        # Check what the actual error is
        ERROR_TEST=$(componentize-py --version 2>&1 || true)
        if echo "$ERROR_TEST" | grep -q "incompatible architecture\|mach-o file"; then
            echo -e "${YELLOW}⚠ componentize-py is installed but has architecture mismatch${NC}"
            if python3 -c "import platform; print(platform.machine())" 2>/dev/null | grep -q "arm64\|aarch64"; then
                echo -e "${YELLOW}  componentize-py is x86_64 but system is ARM64${NC}"
            else
                echo -e "${YELLOW}  componentize-py is ARM64 but system is x86_64${NC}"
            fi
            echo -e "${YELLOW}  Fix: pip uninstall componentize-py && pip install componentize-py${NC}"
        else
            echo -e "${YELLOW}⚠ componentize-py is installed but not working: ${ERROR_TEST}${NC}"
        fi
        echo -e "${YELLOW}  Will create placeholder WASM files instead.${NC}"
        echo ""
    fi
else
    echo -e "${YELLOW}⚠ componentize-py not found.${NC}"
    echo "To install: pip install componentize-py"
    echo "Or download from: https://github.com/bytecodealliance/javy/releases"
    echo ""
    echo -e "${YELLOW}Note: Will create placeholder WASM files for demonstration.${NC}"
    echo ""
fi

# Create wasm-modules directory if it doesn't exist
mkdir -p "$WASM_DIR"

# Track if any builds succeeded
ANY_BUILD_SUCCEEDED=false

# Build each Python actor
cd "$ACTORS_DIR"

for actor_file in *.py; do
    if [ ! -f "$actor_file" ]; then
        continue
    fi
    
    actor_name="${actor_file%.py}"
    wasm_output="$WASM_DIR/${actor_name}.wasm"
    
    echo -e "\n${YELLOW}Building ${actor_name}...${NC}"
    
    # Build using componentize-py (only if it's actually working)
    BUILD_SUCCESS=false
    
    if [ "$COMPONENTIZE_PY_WORKING" = true ]; then
        # Based on actual help output, the correct syntax is:
        # componentize-py --wit-path <WIT_DIR> -w <WORLD> componentize --output <OUTPUT> <APP_NAME>
        # Where:
        #   --wit-path: Directory containing WIT files (use -d as shorthand)
        #   -w/--world: World name (e.g., "plexspaces-actor")
        #   componentize: The subcommand
        #   --output/-o: Output file
        #   <APP_NAME>: Python module name (without .py), must be in current directory or python-path
        
        # Extract app name (without .py extension) - this is the module name
        APP_NAME="${actor_file%.py}"
        WORLD_NAME="plexspaces-actor"
        
        # Method 1: componentize-py --wit-path <wit-dir> -w <world> componentize --output <output> <app-name>
        # This is the correct syntax based on help output
        if [ "$BUILD_SUCCESS" = false ]; then
            # Need to run from the actors directory so Python can find the module
            if (cd "$ACTORS_DIR" && componentize-py --wit-path "$WIT_DIR" -w "$WORLD_NAME" componentize --output "$wasm_output" "$APP_NAME" 2>&1); then
                if [ -f "$wasm_output" ] && [ -s "$wasm_output" ]; then
                    BUILD_SUCCESS=true
                fi
            fi
        fi
        
        # Method 2: componentize-py -d <wit-dir> -w <world> componentize --output <output> <app-name>
        # Using -d shorthand for --wit-path
        if [ "$BUILD_SUCCESS" = false ]; then
            if (cd "$ACTORS_DIR" && componentize-py -d "$WIT_DIR" -w "$WORLD_NAME" componentize --output "$wasm_output" "$APP_NAME" 2>&1); then
                if [ -f "$wasm_output" ] && [ -s "$wasm_output" ]; then
                    BUILD_SUCCESS=true
                fi
            fi
        fi
        
        # Method 3: componentize-py --wit-path <wit-dir> componentize --output <output> <app-name>
        # Try without world name (might use default)
        if [ "$BUILD_SUCCESS" = false ]; then
            if (cd "$ACTORS_DIR" && componentize-py --wit-path "$WIT_DIR" componentize --output "$wasm_output" "$APP_NAME" 2>&1); then
                if [ -f "$wasm_output" ] && [ -s "$wasm_output" ]; then
                    BUILD_SUCCESS=true
                fi
            fi
        fi
        
        # Method 4: componentize-py -d <wit-dir> componentize --output <output> <app-name>
        # Using -d shorthand without world
        if [ "$BUILD_SUCCESS" = false ]; then
            if (cd "$ACTORS_DIR" && componentize-py -d "$WIT_DIR" componentize --output "$wasm_output" "$APP_NAME" 2>&1); then
                if [ -f "$wasm_output" ] && [ -s "$wasm_output" ]; then
                    BUILD_SUCCESS=true
                fi
            fi
        fi
    fi
        
    # Report results
    if [ "$BUILD_SUCCESS" = true ] && [ -f "$wasm_output" ] && [ -s "$wasm_output" ]; then
        # Optimize WASM file using wasm-opt if available
        if command -v wasm-opt &> /dev/null; then
            echo -e "${YELLOW}  Optimizing WASM with wasm-opt...${NC}"
            wasm_opt_output="${wasm_output}.opt"
            if wasm-opt -Oz --strip-debug "$wasm_output" -o "$wasm_opt_output" 2>/dev/null; then
                if [ -f "$wasm_opt_output" ] && [ -s "$wasm_opt_output" ]; then
                    original_size=$(stat -f%z "$wasm_output" 2>/dev/null || stat -c%s "$wasm_output" 2>/dev/null || echo "0")
                    optimized_size=$(stat -f%z "$wasm_opt_output" 2>/dev/null || stat -c%s "$wasm_opt_output" 2>/dev/null || echo "0")
                    if [ "$optimized_size" -lt "$original_size" ]; then
                        mv "$wasm_opt_output" "$wasm_output"
                        reduction=$(( (original_size - optimized_size) * 100 / original_size ))
                        echo -e "${GREEN}  ✓ Optimized: ${original_size} bytes → ${optimized_size} bytes (${reduction}% reduction)${NC}"
                    else
                        rm -f "$wasm_opt_output"
                    fi
                fi
            fi
        else
            echo -e "${YELLOW}  Tip: Install wasm-opt to reduce WASM file size:${NC}"
            echo -e "${YELLOW}    brew install binaryen  # macOS${NC}"
            echo -e "${YELLOW}    apt-get install binaryen  # Linux${NC}"
        fi
        
        final_size=$(stat -f%z "$wasm_output" 2>/dev/null || stat -c%s "$wasm_output" 2>/dev/null || echo "0")
        size_mb=$(( final_size / 1024 / 1024 ))
        echo -e "${GREEN}✓ Built ${actor_name}.wasm (${size_mb}MB)${NC}"
        ANY_BUILD_SUCCEEDED=true
    else
        # componentize-py is not working or build failed
        if [ "$COMPONENTIZE_PY_AVAILABLE" = true ] && [ "$COMPONENTIZE_PY_WORKING" = false ]; then
            # Already reported the issue at the top, just create placeholder
            echo -e "${YELLOW}  Creating placeholder WASM file...${NC}"
        elif [ "$COMPONENTIZE_PY_WORKING" = true ]; then
            # componentize-py is working but build failed - show actual error
            echo -e "${RED}✗ Failed to build ${actor_name}.wasm${NC}"
            # Try to get the actual error using the correct syntax
            APP_NAME="${actor_file%.py}"
            ERROR_OUTPUT=$(cd "$ACTORS_DIR" && componentize-py -d "$WIT_DIR" -w "$WORLD_NAME" componentize --output "$wasm_output" "$APP_NAME" 2>&1 || true)
            if [ -n "$ERROR_OUTPUT" ]; then
                # Extract the key error message (usually the AssertionError or main error)
                ERROR_MSG=$(echo "$ERROR_OUTPUT" | grep -E "AssertionError|Error|Caused by" | head -3 | sed 's/^[[:space:]]*//' | tr '\n' '; ' | sed 's/; $//')
                if [ -z "$ERROR_MSG" ]; then
                    ERROR_MSG=$(echo "$ERROR_OUTPUT" | head -5 | tail -2 | tr '\n' ' ' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')
                fi
                if [ -n "$ERROR_MSG" ]; then
                    echo -e "${RED}  Error: ${ERROR_MSG}${NC}"
                fi
            fi
            echo -e "${YELLOW}  Creating placeholder WASM file for demonstration...${NC}"
        else
            # componentize-py not available
            echo -e "${YELLOW}  componentize-py not available, creating placeholder...${NC}"
        fi
        
        # Create a minimal placeholder WASM file
        echo "00 61 73 6d 01 00 00 00" | xxd -r -p > "$wasm_output" 2>/dev/null || touch "$wasm_output"
        echo -e "${YELLOW}✓ Created placeholder ${actor_name}.wasm (not a real build)${NC}"
    fi
done

# Summary
echo ""
if [ "$ANY_BUILD_SUCCEEDED" = true ]; then
    echo -e "${GREEN}✅ Python calculator actor build complete!${NC}"
    echo "WASM modules: $WASM_DIR"
    exit 0
elif [ "$COMPONENTIZE_PY_WORKING" = true ]; then
    # componentize-py works but all builds failed (likely WIT file issue)
    echo -e "${RED}❌ All builds failed${NC}"
    echo "WASM modules: $WASM_DIR"
    echo -e "${RED}  All files are placeholders - builds failed due to errors (see above)${NC}"
    echo -e "${YELLOW}  Common issues:${NC}"
    echo -e "${YELLOW}    - WIT file parsing errors (check WIT syntax)${NC}"
    echo -e "${YELLOW}    - Python module import errors${NC}"
    echo -e "${YELLOW}    - Missing dependencies${NC}"
    exit 1
elif [ "$COMPONENTIZE_PY_AVAILABLE" = true ]; then
    echo -e "${YELLOW}⚠ Build complete with placeholders (componentize-py not working)${NC}"
    echo "WASM modules: $WASM_DIR"
    echo -e "${YELLOW}  All files are placeholders - fix componentize-py to build real WASM files${NC}"
    echo -e "${YELLOW}  Run: pip uninstall componentize-py && pip install componentize-py${NC}"
    exit 1
else
    echo -e "${YELLOW}⚠ Build complete with placeholders (componentize-py not installed)${NC}"
    echo "WASM modules: $WASM_DIR"
    echo -e "${YELLOW}  Install componentize-py to build real WASM files: pip install componentize-py${NC}"
    exit 1
fi






















































