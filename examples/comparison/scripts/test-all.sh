#!/bin/bash
# Comprehensive test script for all comparison examples
# Verifies all examples build, test, and execute correctly

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPARISON_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$COMPARISON_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  PlexSpaces Framework Comparison Examples - Test Suite        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Track results
PASSED=0
FAILED=0
SKIPPED=0
TOTAL=0

# List of all comparison examples (alphabetical order)
EXAMPLES=(
    "aws_durable_lambda"
    "aws_step_functions"
    "azure_durable_functions"
    "cadence"
    "cloudflare_workers"
    "conductor"
    "dapr"
    "dirigo"
    "eflows4hpc"
    "erlang_otp"
    "gosiris"
    "kueue"
    "luats"
    "merlin"
    "mozartspaces"
    "orbit"
    "orleans"
    "ractor"
    "ray"
    "restate"
    "rivet"
    "skypilot"
    "temporal"
    "wasmcloud"
    "zeebe"
)

echo "Found ${#EXAMPLES[@]} comparison examples to test"
echo ""

# Test each example
for example in "${EXAMPLES[@]}"; do
    TOTAL=$((TOTAL + 1))
    EXAMPLE_DIR="$COMPARISON_DIR/$example"
    TEST_SCRIPT="$EXAMPLE_DIR/scripts/test.sh"
    
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${BLUE}Testing: ${example}${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # Check if example directory exists
    if [ ! -d "$EXAMPLE_DIR" ]; then
        echo -e "${YELLOW}⚠️  Example directory not found: ${example}${NC}"
        SKIPPED=$((SKIPPED + 1))
        echo ""
        continue
    fi
    
    # Check if test script exists
    if [ ! -f "$TEST_SCRIPT" ]; then
        echo -e "${YELLOW}⚠️  Test script not found: ${TEST_SCRIPT}${NC}"
        SKIPPED=$((SKIPPED + 1))
        echo ""
        continue
    fi
    
    # Check if test script is executable
    if [ ! -x "$TEST_SCRIPT" ]; then
        echo -e "${YELLOW}⚠️  Test script not executable, making it executable...${NC}"
        chmod +x "$TEST_SCRIPT"
    fi
    
    # Run test script
    if bash "$TEST_SCRIPT" 2>&1 | tee "/tmp/${example}_test.log"; then
        if grep -q "All tests passed\|✅ All tests passed" "/tmp/${example}_test.log"; then
            echo -e "${GREEN}✅ ${example}: PASSED${NC}"
            PASSED=$((PASSED + 1))
        else
            echo -e "${RED}❌ ${example}: FAILED (no success message)${NC}"
            FAILED=$((FAILED + 1))
        fi
    else
        echo -e "${RED}❌ ${example}: FAILED (test script error)${NC}"
        FAILED=$((FAILED + 1))
    fi
    echo ""
done

# Summary
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║  Test Summary                                                  ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "Total examples: ${TOTAL}"
echo -e "${GREEN}Passed: ${PASSED}${NC}"
echo -e "${RED}Failed: ${FAILED}${NC}"
echo -e "${YELLOW}Skipped: ${SKIPPED}${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}╔════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║  ✅ All tests passed!                                          ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════════════════════════╝${NC}"
    exit 0
else
    echo -e "${RED}╔════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${RED}║  ❌ Some tests failed                                          ║${NC}"
    echo -e "${RED}╚════════════════════════════════════════════════════════════════╝${NC}"
    exit 1
fi
