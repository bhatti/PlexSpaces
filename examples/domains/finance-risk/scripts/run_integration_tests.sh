#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${BLUE}=== Running Finance Risk Assessment Integration Tests ===${NC}"
echo ""

cd "$PROJECT_DIR"

# Parse arguments
FILTER=""
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --filter)
            FILTER="$2"
            shift 2
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --filter <name>    Run only tests matching name"
            echo "  --verbose          Show detailed test output"
            echo "  --help             Show this help message"
            echo ""
            echo "Examples:"
            echo "  $0                                    # Run all integration tests"
            echo "  $0 --filter coordinator_crash        # Run only coordinator crash tests"
            echo "  $0 --verbose                          # Run with detailed output"
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            echo "Run with --help for usage information"
            exit 1
            ;;
    esac
done

# Build test binaries
echo -e "${BLUE}Building integration test binaries...${NC}"
cargo build --tests --lib

echo ""
echo -e "${GREEN}=== Test Suite Overview ===${NC}"
echo "  1. Coordinator Crash Recovery (4 tests)"
echo "     - Crash during risk scoring"
echo "     - Crash during manual review"
echo "     - Crash with cached API results"
echo "     - Multiple crashes"
echo ""
echo "  2. Worker Failure Recovery (5 tests)"
echo "     - Credit check worker crash"
echo "     - Risk scoring worker crash"
echo "     - Multiple data collection workers crash"
echo "     - Notification worker crash"
echo "     - Worker crash with API timeout"
echo ""
echo "  3. Concurrent Applications (6 tests)"
echo "     - 10 loan applications concurrent processing"
echo "     - Worker pool sharing"
echo "     - No data leakage"
echo "     - External API rate limiting"
echo "     - Throughput measurement"
echo "     - Performance degradation check"
echo ""

# Run tests
FAILED=0

run_test_suite() {
    local test_name=$1
    local test_file=$2

    echo -e "${BLUE}Running $test_name...${NC}"

    if [ -n "$FILTER" ]; then
        if [[ ! "$test_name" =~ $FILTER ]]; then
            echo -e "${YELLOW}Skipped (filter: $FILTER)${NC}"
            return 0
        fi
    fi

    if [ "$VERBOSE" = true ]; then
        cargo test --test "$test_file" -- --ignored --nocapture || FAILED=$((FAILED + 1))
    else
        cargo test --test "$test_file" -- --ignored || FAILED=$((FAILED + 1))
    fi

    echo ""
}

# Run test suites
run_test_suite "Coordinator Crash Recovery" "coordinator_crash_recovery"
run_test_suite "Worker Failure Recovery" "worker_failure_recovery"
run_test_suite "Concurrent Applications" "concurrent_applications"

# Summary
echo -e "${GREEN}=== Test Summary ===${NC}"
if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✅ All integration tests passed!${NC}"
    exit 0
else
    echo -e "${RED}❌ $FAILED test suite(s) failed${NC}"
    exit 1
fi
