#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Validation script for WASM Showcase Example

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  WASM Showcase Example Validation                              ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

TESTS_PASSED=0
TESTS_FAILED=0

function pass() {
    echo -e "${GREEN}✓ PASS${NC}: $1"
    ((TESTS_PASSED++))
}

function fail() {
    echo -e "${RED}✗ FAIL${NC}: $1"
    ((TESTS_FAILED++))
}

function info() {
    echo -e "${BLUE}ℹ INFO${NC}: $1"
}

# Test 1: Build example
echo -e "${YELLOW}Test 1: Building example...${NC}"
cd "$EXAMPLE_DIR"
if cargo build --release 2>&1 | tail -5 | grep -q "Finished"; then
    pass "Example builds successfully"
else
    fail "Example build failed"
    exit 1
fi

# Test 2: Run deployment demo
echo -e "\n${YELLOW}Test 2: Running deployment demo...${NC}"
if timeout 30 cargo run --release --bin wasm-showcase -- --demo deployment 2>&1 | grep -q "Deployment demo complete"; then
    pass "Deployment demo completed"
else
    fail "Deployment demo failed"
fi

# Test 3: Run security demo
echo -e "\n${YELLOW}Test 3: Running security demo...${NC}"
if timeout 30 cargo run --release --bin wasm-showcase -- --demo security 2>&1 | grep -q "Security demo complete"; then
    pass "Security demo completed"
else
    fail "Security demo failed"
fi

# Test 4: Run Python demo
echo -e "\n${YELLOW}Test 4: Running Python demo...${NC}"
if timeout 30 cargo run --release --bin wasm-showcase -- --demo python 2>&1 | grep -q "Python demo complete"; then
    pass "Python demo completed"
else
    fail "Python demo failed"
fi

# Test 5: Run behaviors demo
echo -e "\n${YELLOW}Test 5: Running behaviors demo...${NC}"
if timeout 30 cargo run --release --bin wasm-showcase -- --demo behaviors 2>&1 | grep -q "Behaviors demo complete"; then
    pass "Behaviors demo completed"
else
    fail "Behaviors demo failed"
fi

# Test 6: Run services demo
echo -e "\n${YELLOW}Test 6: Running services demo...${NC}"
if timeout 30 cargo run --release --bin wasm-showcase -- --demo services 2>&1 | grep -q "Services demo complete"; then
    pass "Services demo completed"
else
    fail "Services demo failed"
fi

# Test 7: Check Python actors exist
echo -e "\n${YELLOW}Test 7: Checking Python actors...${NC}"
if [ -f "$EXAMPLE_DIR/actors/python/counter_actor.py" ] && \
   [ -f "$EXAMPLE_DIR/actors/python/event_actor.py" ] && \
   [ -f "$EXAMPLE_DIR/actors/python/fsm_actor.py" ] && \
   [ -f "$EXAMPLE_DIR/actors/python/service_demo_actor.py" ]; then
    pass "All Python actor examples exist"
else
    fail "Some Python actor examples missing"
fi

# Test 8: Check scripts exist
echo -e "\n${YELLOW}Test 8: Checking scripts...${NC}"
if [ -x "$SCRIPT_DIR/run.sh" ] && \
   [ -x "$SCRIPT_DIR/build_python_actors.sh" ] && \
   [ -x "$SCRIPT_DIR/e2e_test.sh" ]; then
    pass "All scripts exist and are executable"
else
    fail "Some scripts missing or not executable"
fi

# Test 9: Check README exists
echo -e "\n${YELLOW}Test 9: Checking documentation...${NC}"
if [ -f "$EXAMPLE_DIR/README.md" ]; then
    # Check for key sections
    if grep -q "Python Actor Support" "$EXAMPLE_DIR/README.md" && \
       grep -q "Actor Behaviors" "$EXAMPLE_DIR/README.md" && \
       grep -q "Service Access" "$EXAMPLE_DIR/README.md"; then
        pass "README is comprehensive"
    else
        fail "README missing key sections"
    fi
else
    fail "README.md not found"
fi

# Summary
echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}Validation Summary${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "Tests Passed: ${GREEN}${TESTS_PASSED}${NC}"
echo -e "Tests Failed: ${RED}${TESTS_FAILED}${NC}"
echo -e "Total Tests:  $((TESTS_PASSED + TESTS_FAILED))"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}✅ All validation tests passed!${NC}"
    exit 0
else
    echo -e "${RED}❌ Some validation tests failed${NC}"
    exit 1
fi

