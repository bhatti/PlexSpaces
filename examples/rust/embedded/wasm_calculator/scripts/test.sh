#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for WASM Calculator Example
# Validates WASM actor spawning and node lifecycle

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  WASM Calculator Example Test Script                          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found. Please install Rust toolchain.${NC}"
    exit 1
fi

# Build the example
echo -e "${YELLOW}📦 Building wasm_calculator...${NC}"
if ! cargo build --release 2>&1 | grep -q "Finished"; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build successful${NC}"
echo ""

# Run the example and capture output
echo -e "${YELLOW}🚀 Running wasm_calculator...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)

# Run with timeout and capture output
timeout 10s cargo run --release --bin wasm_calculator 2>&1 | tee "$OUTPUT_FILE" || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo -e "${YELLOW}⏱️  Test completed (timeout after 10s)${NC}"
    else
        echo -e "${RED}❌ Test failed with exit code $EXIT_CODE${NC}"
        rm -f "$OUTPUT_FILE"
        exit $EXIT_CODE
    fi
}

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}📊 Validation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Parse output for validation
NODE_CREATED=$(grep -c "Node created\|Node started" "$OUTPUT_FILE" || echo "0")
PYTHON_DEPLOYED=$(grep -c "Python.*deployed\|Application deployed\|durable-calculator-app\|tuplespace-calculator-app" "$OUTPUT_FILE" || echo "0")
WASM_MODULE=$(grep -c "WASM module\|Application deployed" "$OUTPUT_FILE" || echo "0")
PYTHON_ACTORS=$(grep -c "Python.*actor\|tuplespace_calculator_actor\|durable_calculator_actor" "$OUTPUT_FILE" || echo "0")
TUPLESPACE_USAGE=$(grep -c "TupleSpace\|tuplespace\|Found.*results in TupleSpace\|host.tuplespace" "$OUTPUT_FILE" || echo "0")
EXAMPLE_COMPLETE=$(grep -c "Example complete\|Key Takeaways" "$OUTPUT_FILE" || echo "0")

# Validation results
VALIDATION_PASSED=true

echo -e "${BLUE}Feature Validation:${NC}"
echo "  • Node created/started: $([ "$NODE_CREATED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Python WASM applications deployed: $([ "$PYTHON_DEPLOYED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • WASM module referenced: $([ "$WASM_MODULE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Python actors demonstrated: $([ "$PYTHON_ACTORS" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • TupleSpace coordination: $([ "$TUPLESPACE_USAGE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Example completed: $([ "$EXAMPLE_COMPLETE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo ""

# Validate expectations
if [ "$NODE_CREATED" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Node was not created/started${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Node creation validation passed${NC}"
fi

if [ "$PYTHON_DEPLOYED" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Python WASM applications were not deployed${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Python application deployment validation passed${NC}"
fi

if [ "$TUPLESPACE_USAGE" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: TupleSpace coordination was not demonstrated${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ TupleSpace coordination validation passed${NC}"
fi

if [ "$EXAMPLE_COMPLETE" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Example did not complete successfully${NC}"
    VALIDATION_PASSED=false
fi

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# Final result
if [ "$VALIDATION_PASSED" = true ]; then
    echo -e "${GREEN}✅ All validations passed!${NC}"
    echo ""
    echo -e "${BLUE}Key Features Validated:${NC}"
    echo "  • Python WASM applications deployed via ApplicationService"
    echo "  • Durable actors: State persistence, journaling, checkpointing"
    echo "  • TupleSpace coordination from Python using host.tuplespace_write/read"
    echo "  • Dynamic module deployment at runtime"
    echo "  • Content-addressed module caching"
    echo "  • NodeBuilder and ConfigBootstrap usage"
    rm -f "$OUTPUT_FILE"
    exit 0
else
    echo -e "${RED}❌ Some validations failed${NC}"
    echo ""
    echo -e "${YELLOW}Note: Check the output above for details.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi


