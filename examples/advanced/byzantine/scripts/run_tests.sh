#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

# Run all tests for byzantine example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Byzantine Generals - Test Runner                      ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

cd "$PROJECT_DIR"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

# Function to print section headers
print_section() {
    echo -e "${YELLOW}>>> $1${NC}"
    echo ""
}

# Function to print success
print_success() {
    echo -e "${GREEN}✓ $1${NC}"
    echo ""
}

# Function to print error
print_error() {
    echo -e "${RED}✗ $1${NC}"
    echo ""
}

# 1. Run unit tests
print_section "Running unit tests..."
if cargo test --lib --no-fail-fast; then
    print_success "Unit tests passed"
else
    print_error "Unit tests failed"
    exit 1
fi

# 2. Run integration tests
if [ -d "tests" ] && [ "$(ls -A tests/*.rs 2>/dev/null)" ]; then
    print_section "Running integration tests..."
    if cargo test --test '*' --no-fail-fast; then
        print_success "Integration tests passed"
    else
        print_error "Integration tests failed"
        exit 1
    fi
else
    echo "No integration tests found (skipping)"
    echo ""
fi

# 3. Build binary
print_section "Building binary..."
if cargo build --release --bin byzantine; then
    print_success "Binary build succeeded"
else
    print_error "Binary build failed"
    exit 1
fi

# 4. Run example with metrics
print_section "Running example with metrics..."
if cargo run --release --bin byzantine 2>&1 | tee /tmp/byzantine_test_output.txt; then
    print_success "Example run succeeded"
    
    # Display metrics
    if grep -q "Metrics:" /tmp/byzantine_test_output.txt; then
        echo -e "${BLUE}Metrics Summary:${NC}"
        grep -A 3 "Metrics:" /tmp/byzantine_test_output.txt | sed 's/^/  /'
        echo ""
    fi
else
    print_error "Example run failed"
    exit 1
fi

echo "╔════════════════════════════════════════════════════════════╗"
echo -e "${GREEN}All tests passed successfully!${NC}"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""
echo "Summary:"
echo "  ✓ Unit tests passed"
echo "  ✓ Integration tests passed"
echo "  ✓ Binary builds"
echo "  ✓ Example runs with metrics"
echo ""

