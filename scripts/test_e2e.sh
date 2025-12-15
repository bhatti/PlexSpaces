#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# End-to-End Testing Script for PlexSpaces
#
# This script runs comprehensive E2E tests across the entire PlexSpaces framework,
# including:
# - Unit tests for all crates
# - Integration tests
# - Byzantine Generals distributed consensus
# - TupleSpace coordination tests
# - Actor communication tests
#
# Usage:
#   ./scripts/test_e2e.sh [OPTIONS]
#
# Options:
#   --quick         Skip slow tests (< 30 seconds total)
#   --coverage      Generate coverage report
#   --features      Comma-separated features (e.g., sql-backend,redis-backend)
#   --verbose       Show detailed output
#   --no-fail-fast  Continue testing after first failure
#   --help          Show this help message

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default options
QUICK_MODE=false
COVERAGE=false
FEATURES=""
VERBOSE=false
FAIL_FAST=true
SHOW_HELP=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            QUICK_MODE=true
            shift
            ;;
        --coverage)
            COVERAGE=true
            shift
            ;;
        --features)
            FEATURES="$2"
            shift 2
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --no-fail-fast)
            FAIL_FAST=false
            shift
            ;;
        --help)
            SHOW_HELP=true
            shift
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            SHOW_HELP=true
            shift
            ;;
    esac
done

if [ "$SHOW_HELP" = true ]; then
    grep "^#" "$0" | grep -v "#!/" | sed 's/^# //'
    exit 0
fi

# Print header
echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  PlexSpaces End-to-End Test Suite                         ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Get project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# Test counters
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# Function to run a test phase
run_test_phase() {
    local phase_name="$1"
    local phase_desc="$2"
    local test_cmd="$3"

    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}▶ Phase: $phase_name${NC}"
    echo -e "  $phase_desc"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    if [ "$VERBOSE" = true ]; then
        echo -e "${BLUE}Command: $test_cmd${NC}"
        echo ""
    fi

    # Run the test
    START_TIME=$(date +%s)

    if eval "$test_cmd"; then
        END_TIME=$(date +%s)
        DURATION=$((END_TIME - START_TIME))
        echo ""
        echo -e "${GREEN}✅ PASSED${NC} ($DURATION seconds)"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        END_TIME=$(date +%s)
        DURATION=$((END_TIME - START_TIME))
        echo ""
        echo -e "${RED}❌ FAILED${NC} ($DURATION seconds)"
        FAILED_TESTS=$((FAILED_TESTS + 1))

        if [ "$FAIL_FAST" = true ]; then
            echo -e "${RED}Aborting due to test failure (use --no-fail-fast to continue)${NC}"
            exit 1
        fi
    fi

    echo ""
}

# Build feature flags
FEATURE_FLAGS=""
if [ -n "$FEATURES" ]; then
    FEATURE_FLAGS="--features $FEATURES"
fi

FAIL_FAST_FLAG=""
if [ "$FAIL_FAST" = false ]; then
    FAIL_FAST_FLAG="--no-fail-fast"
fi

echo -e "${BLUE}Configuration:${NC}"
echo -e "  Quick Mode: $QUICK_MODE"
echo -e "  Coverage: $COVERAGE"
echo -e "  Features: ${FEATURES:-none}"
echo -e "  Verbose: $VERBOSE"
echo -e "  Fail Fast: $FAIL_FAST"
echo ""

# Phase 1: Workspace Build
run_test_phase \
    "Build" \
    "Building entire workspace" \
    "cargo build --workspace --lib $FEATURE_FLAGS"

# Phase 2: Core Crate Tests
run_test_phase \
    "Unit Tests - Core Crates" \
    "Testing proto, lattice, mailbox, persistence, tuplespace, facet, core, behavior" \
    "cargo test --lib -p plexspaces-proto -p plexspaces-lattice -p plexspaces-mailbox -p plexspaces-persistence -p plexspaces-tuplespace -p plexspaces-facet -p plexspaces-core -p plexspaces-behavior $FAIL_FAST_FLAG $FEATURE_FLAGS"

# Phase 3: Actor & Supervision Tests
run_test_phase \
    "Unit Tests - Actor System" \
    "Testing actor, supervisor, node" \
    "cargo test --lib -p plexspaces-actor -p plexspaces-supervisor -p plexspaces-node $FAIL_FAST_FLAG $FEATURE_FLAGS"

# Phase 4: TupleSpace with SQL Backend
if [ -n "$FEATURES" ] && [[ "$FEATURES" == *"sql-backend"* ]]; then
    run_test_phase \
        "Integration Tests - SQL Backend" \
        "Testing SQLite and PostgreSQL backends" \
        "cargo test --lib -p plexspaces-tuplespace --features sql-backend $FAIL_FAST_FLAG"
fi

# Phase 5: Byzantine Generals E2E Tests
if [ "$QUICK_MODE" = false ]; then
    run_test_phase \
        "E2E - Byzantine Generals (Local)" \
        "Testing Byzantine consensus with local actors" \
        "cd examples/byzantine && cargo test --test basic_consensus $FAIL_FAST_FLAG"

    run_test_phase \
        "E2E - Byzantine Generals (Distributed TupleSpace)" \
        "Testing Byzantine consensus with distributed coordination" \
        "cd examples/byzantine && cargo test --test distributed_tuplespace_test $FAIL_FAST_FLAG"
fi

# Phase 6: Workspace Root Tests
run_test_phase \
    "Integration Tests - Workspace" \
    "Testing cross-crate integration" \
    "cargo test --lib --tests $FAIL_FAST_FLAG $FEATURE_FLAGS"

# Phase 7: Coverage Report (if requested)
if [ "$COVERAGE" = true ]; then
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}▶ Generating Coverage Report${NC}"
    echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""

    if ! command -v cargo-tarpaulin &> /dev/null; then
        echo -e "${YELLOW}Installing cargo-tarpaulin...${NC}"
        cargo install cargo-tarpaulin
    fi

    cargo tarpaulin --lib --all-features --out Html --output-dir coverage
    echo -e "${GREEN}Coverage report generated: coverage/index.html${NC}"
    echo ""
fi

# Print summary
echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}Test Summary${NC}"
echo -e "${YELLOW}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "  Total Test Phases: $TOTAL_TESTS"
echo -e "  ${GREEN}Passed: $PASSED_TESTS${NC}"
echo -e "  ${RED}Failed: $FAILED_TESTS${NC}"
echo ""

if [ $FAILED_TESTS -eq 0 ]; then
    echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║  ✅ ALL TESTS PASSED!                                      ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
    exit 0
else
    echo -e "${RED}╔════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${RED}║  ❌ SOME TESTS FAILED                                      ║${NC}"
    echo -e "${RED}╚════════════════════════════════════════════════════════════╝${NC}"
    exit 1
fi
