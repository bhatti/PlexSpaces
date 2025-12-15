#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

# Run all tests for order-processing example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=========================================="
echo "PlexSpaces Order Processing - Test Runner"
echo "=========================================="
echo ""

cd "$PROJECT_DIR"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

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

# 2. Check test coverage
print_section "Checking test coverage..."
if cargo tarpaulin --lib --timeout 120 --skip-clean 2>&1 | tee /tmp/coverage_output.txt; then
    # Extract coverage percentage
    COVERAGE=$(grep "coverage" /tmp/coverage_output.txt | tail -1 | grep -oP '\d+\.\d+(?=%)')

    if [ -n "$COVERAGE" ]; then
        echo "Coverage: $COVERAGE%"

        # Check if coverage meets threshold
        if (( $(echo "$COVERAGE >= 90" | bc -l) )); then
            print_success "Coverage threshold met: $COVERAGE% >= 90%"
        else
            print_error "Coverage threshold not met: $COVERAGE% < 90%"
            exit 1
        fi
    else
        print_error "Could not extract coverage percentage"
        exit 1
    fi
else
    print_error "Coverage check failed"
    exit 1
fi

# 3. Run clippy
print_section "Running clippy..."
if cargo clippy --lib -- -D warnings; then
    print_success "Clippy passed"
else
    print_error "Clippy failed"
    exit 1
fi

# 4. Check formatting
print_section "Checking code formatting..."
if cargo fmt -- --check; then
    print_success "Code formatting is correct"
else
    print_error "Code formatting issues found. Run 'cargo fmt' to fix"
    exit 1
fi

# 5. Build binary
print_section "Building binary..."
if cargo build --bin order-coordinator; then
    print_success "Binary build succeeded"
else
    print_error "Binary build failed"
    exit 1
fi

# 6. Run integration tests (if any)
if [ -d "tests" ] && [ "$(ls -A tests/*.rs 2>/dev/null)" ]; then
    print_section "Running integration tests..."
    if cargo test --test '*'; then
        print_success "Integration tests passed"
    else
        print_error "Integration tests failed"
        exit 1
    fi
else
    echo "No integration tests found (skipping)"
    echo ""
fi

echo "=========================================="
echo -e "${GREEN}All tests passed successfully!${NC}"
echo "=========================================="
echo ""
echo "Summary:"
echo "  ✓ Unit tests passed"
echo "  ✓ Coverage >= 90%"
echo "  ✓ Clippy passed"
echo "  ✓ Formatting correct"
echo "  ✓ Binary builds"
echo ""
