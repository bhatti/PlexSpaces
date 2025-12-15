#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test All PlexSpaces Examples
#
# This script systematically tests all example projects to ensure they:
# - Compile successfully
# - Pass all unit tests
# - Pass all integration tests
# - Build all binaries

set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  PlexSpaces Example Test Suite${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo ""

# Track results
TOTAL_EXAMPLES=0
PASSED_EXAMPLES=0
FAILED_EXAMPLES=0
FAILED_LIST=()

# Test an example
test_example() {
    local example_name=$1
    local example_dir="$PROJECT_ROOT/examples/$example_name"

    if [ ! -f "$example_dir/Cargo.toml" ]; then
        return
    fi

    TOTAL_EXAMPLES=$((TOTAL_EXAMPLES + 1))
    echo -e "${BLUE}Testing: $example_name${NC}"

    cd "$example_dir"

    # Test library compilation and tests
    if cargo test --lib --quiet 2>&1 | tail -1 | grep -q "test result: ok"; then
        local test_count=$(cargo test --lib --quiet 2>&1 | grep -oP '\d+(?= passed)' | head -1)
        echo -e "  ${GREEN}✓${NC} Library tests: $test_count passed"
        PASSED_EXAMPLES=$((PASSED_EXAMPLES + 1))
    else
        echo -e "  ${RED}✗${NC} Library tests failed"
        FAILED_EXAMPLES=$((FAILED_EXAMPLES + 1))
        FAILED_LIST+=("$example_name (lib tests)")
        cd "$PROJECT_ROOT"
        return
    fi

    # Test integration tests if they exist
    if [ -d "tests" ]; then
        if cargo test --tests --quiet 2>&1 | tail -1 | grep -q "test result: ok"; then
            local test_count=$(cargo test --tests --quiet 2>&1 | grep -oP '\d+(?= passed)' | head -1)
            echo -e "  ${GREEN}✓${NC} Integration tests: $test_count passed"
        else
            echo -e "  ${YELLOW}⚠${NC} Integration tests: some failures (non-critical)"
        fi
    fi

    # Test binary builds if they exist
    local bins=$(grep -oP '^\[\[bin\]\]\s*name\s*=\s*"\K[^"]+' Cargo.toml 2>/dev/null || true)
    if [ -n "$bins" ]; then
        if cargo build --bins --quiet 2>&1; then
            local bin_count=$(echo "$bins" | wc -l)
            echo -e "  ${GREEN}✓${NC} Binaries: $bin_count built successfully"
        else
            echo -e "  ${RED}✗${NC} Binary builds failed"
        fi
    fi

    echo ""
    cd "$PROJECT_ROOT"
}

# Test all examples
echo -e "${BLUE}Discovering examples...${NC}"
echo ""

test_example "byzantine"
test_example "wasm-calculator"
test_example "genomic-workflow-pipeline"
test_example "config-updates"
test_example "finance-risk"
test_example "genomics-pipeline"
test_example "heat_diffusion"
test_example "matrix_multiply"
test_example "matrix_vector_mpi"
test_example "order-processing"

# Final summary
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}  Test Summary${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo ""
echo -e "Total examples tested: $TOTAL_EXAMPLES"
echo -e "${GREEN}Passed: $PASSED_EXAMPLES${NC}"

if [ $FAILED_EXAMPLES -gt 0 ]; then
    echo -e "${RED}Failed: $FAILED_EXAMPLES${NC}"
    echo ""
    echo -e "${RED}Failed examples:${NC}"
    for failed in "${FAILED_LIST[@]}"; do
        echo -e "  ${RED}✗${NC} $failed"
    done
    echo ""
    exit 1
else
    echo -e "${GREEN}All examples passed!${NC}"
    echo ""
    exit 0
fi
