#!/bin/bash
# Byzantine Generals Integration Testing Script
#
# This script provides a repeatable process for running integration tests
# for the Byzantine Generals distributed consensus example.
#
# Usage:
#   ./run-integration-tests.sh              # Run all integration tests
#   ./run-integration-tests.sh --quick      # Run quick tests only
#   ./run-integration-tests.sh --help       # Show help

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print colored message
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Test counters
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

# Run a test phase and track results
run_test_phase() {
    local phase_name="$1"
    local test_command="$2"

    echo ""
    log_info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_info "▶ $phase_name"
    log_info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    if eval "$test_command"; then
        log_success "$phase_name PASSED"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        log_error "$phase_name FAILED"
        FAILED_TESTS=$((FAILED_TESTS + 1))
        return 1
    fi
}

# Run Phase 1: Basic Consensus (in-memory, single process)
run_phase1_tests() {
    run_test_phase \
        "Phase 1: Basic Consensus (In-Memory)" \
        "cargo test --test basic_consensus --no-fail-fast"
}

# Run Phase 2: Distributed TupleSpace (SQLite-backed, multi-node)
run_phase2_tests() {
    run_test_phase \
        "Phase 2: Distributed TupleSpace (SQLite Backend)" \
        "cargo test --test distributed_tuplespace_test --no-fail-fast"
}

# Run Phase 3: Remote Consensus (gRPC-based, multi-node)
run_phase3_tests() {
    run_test_phase \
        "Phase 3: Remote Consensus (gRPC Multi-Node)" \
        "cargo test --test remote_consensus --no-fail-fast"
}

# Run Phase 4: Multi-Process Consensus (requires binary build)
run_phase4_tests() {
    log_info "Building Byzantine binary..."
    if ! cargo build --release; then
        log_error "Failed to build Byzantine binary"
        return 1
    fi

    run_test_phase \
        "Phase 4: Multi-Process Consensus" \
        "cargo test --test multi_process_consensus --no-fail-fast"
}

# Run Phase 5: Distributed Consensus (Redis backend - if available)
run_phase5_tests() {
    # Check if redis-backend feature is enabled
    if ! grep -q 'redis-backend' Cargo.toml; then
        log_warning "Redis backend feature not enabled, skipping Phase 5"
        return 0
    fi

    # NOTE: We use Docker for Redis, not CLI installation
    # Check if Docker is available
    if ! command -v docker &> /dev/null; then
        log_warning "Docker not found, skipping Redis tests"
        log_info "Install Docker to run Redis-based distributed tests"
        return 0
    fi

    # Check if docker-compose is available
    if ! command -v docker-compose &> /dev/null; then
        log_warning "docker-compose not found, skipping Redis tests"
        log_info "Install docker-compose to run Redis-based distributed tests"
        return 0
    fi

    # Check if Redis container is running via docker-compose
    if ! docker-compose ps redis | grep -q "Up"; then
        log_warning "Redis container not running, skipping Redis tests"
        log_info "To start Redis: docker-compose up -d redis"
        log_info "To check status: docker-compose ps redis"
        return 0
    fi

    # Test Redis connection via docker-compose exec
    if ! docker-compose exec -T redis redis-cli ping > /dev/null 2>&1; then
        log_warning "Cannot connect to Redis container, skipping Redis tests"
        log_info "Check Redis container health: docker-compose logs redis"
        return 0
    fi

    log_success "Redis container is ready (via docker-compose)"

    run_test_phase \
        "Phase 5: Distributed Consensus (Redis Backend)" \
        "cargo test --features redis-backend --test distributed_consensus -- --ignored --nocapture"
}

# Run all unit tests
run_unit_tests() {
    run_test_phase \
        "Unit Tests: Byzantine Generals Library" \
        "cargo test --lib --no-fail-fast"
}

# Run coverage analysis
run_coverage() {
    log_info "Running test coverage analysis..."

    if ! command -v cargo-tarpaulin &> /dev/null; then
        log_warning "cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin"
        return 0
    fi

    echo ""
    cargo tarpaulin --lib --tests --timeout 120 --out Stdout | grep -E "coverage|lines|statements"
    echo ""
    log_success "Coverage analysis complete"
}

# Print test summary
print_summary() {
    echo ""
    log_info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_info "Test Summary"
    log_info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  Total Test Phases: $TOTAL_TESTS"
    echo -e "  ${GREEN}Passed: $PASSED_TESTS${NC}"
    echo -e "  ${RED}Failed: $FAILED_TESTS${NC}"
    echo ""

    if [ $FAILED_TESTS -eq 0 ]; then
        log_success "╔════════════════════════════════════════════════════╗"
        log_success "║  ✅ ALL TESTS PASSED!                              ║"
        log_success "╚════════════════════════════════════════════════════╝"
        return 0
    else
        log_error "╔════════════════════════════════════════════════════╗"
        log_error "║  ❌ SOME TESTS FAILED                              ║"
        log_error "╚════════════════════════════════════════════════════╝"
        return 1
    fi
}

# Main execution
main() {
    local quick_mode=false
    local coverage_mode=false

    # Parse arguments
    for arg in "$@"; do
        case $arg in
            --quick)
                quick_mode=true
                ;;
            --coverage)
                coverage_mode=true
                ;;
            --help)
                cat << EOF

Byzantine Generals Integration Testing Script

Usage:
  $0 [OPTIONS]

Options:
  (none)         Run full integration test suite (all phases)
  --quick        Run quick tests only (Phase 1 + Phase 2)
  --coverage     Run tests with coverage analysis
  --help         Show this help message

Test Phases:
  Phase 1: Basic Consensus (in-memory, single process)
  Phase 2: Distributed TupleSpace (SQLite backend, multi-node)
  Phase 3: Remote Consensus (gRPC-based, multi-node)
  Phase 4: Multi-Process Consensus (separate processes)
  Phase 5: Distributed Consensus (Redis via Docker, if available)

Examples:
  # Full integration test suite
  $0

  # Quick tests (Phases 1-2 only)
  $0 --quick

  # With coverage analysis
  $0 --coverage

Directory Structure:
  tests/basic_consensus.rs              - Phase 1 tests
  tests/distributed_tuplespace_test.rs  - Phase 2 tests
  tests/remote_consensus.rs             - Phase 3 tests
  tests/multi_process_consensus.rs      - Phase 4 tests
  tests/distributed_consensus.rs        - Phase 5 tests (Redis)

Quick Start:
  # Run all available tests
  cargo test --no-fail-fast

  # Run specific phase
  cargo test --test basic_consensus
  cargo test --test distributed_tuplespace_test
  cargo test --test remote_consensus

  # Run with output
  cargo test --test distributed_tuplespace_test -- --nocapture

Redis Setup (for Phase 5):
  # Start Redis via Docker Compose
  docker-compose up -d redis

  # Check Redis status
  docker-compose ps redis

  # View Redis logs
  docker-compose logs redis

  # Stop Redis
  docker-compose down

Note: We use Docker for Redis, not local CLI installation.

EOF
                exit 0
                ;;
            *)
                log_error "Unknown option: $arg"
                log_info "Use --help for usage information"
                exit 1
                ;;
        esac
    done

    echo ""
    log_info "╔════════════════════════════════════════════════════════════╗"
    log_info "║  Byzantine Generals Integration Testing                   ║"
    log_info "╚════════════════════════════════════════════════════════════╝"
    echo ""

    # Always run unit tests first
    run_unit_tests || true

    # Phase 1: Basic consensus (always run)
    run_phase1_tests || true

    # Phase 2: Distributed TupleSpace (always run)
    run_phase2_tests || true

    # If quick mode, skip remaining phases
    if [ "$quick_mode" = true ]; then
        log_info "Quick mode enabled, skipping Phases 3-5"
    else
        # Phase 3: Remote consensus
        run_phase3_tests || true

        # Phase 4: Multi-process consensus
        run_phase4_tests || true

        # Phase 5: Redis-based consensus (optional)
        run_phase5_tests || true
    fi

    # Run coverage if requested
    if [ "$coverage_mode" = true ]; then
        echo ""
        run_coverage
    fi

    # Print summary
    print_summary
}

# Run main function
main "$@"
