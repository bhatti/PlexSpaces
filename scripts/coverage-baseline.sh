#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Coverage baseline script for Phase 1 Foundation Audit
# Uses cargo-llvm-cov (supports Rust edition 2024)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Coverage Baseline Analysis                                    ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo

# Check if cargo-llvm-cov is installed
if ! command -v cargo-llvm-cov &> /dev/null; then
    echo -e "${RED}❌ cargo-llvm-cov not found${NC}"
    echo "   Install with: cargo install cargo-llvm-cov"
    exit 1
fi

echo -e "${GREEN}✓ cargo-llvm-cov found${NC}"
echo

# Create coverage directory
mkdir -p target/coverage/html
mkdir -p target/coverage/reports

# List of crates to analyze (excluding proto - generated code)
CRATES=(
    "plexspaces-core"
    "plexspaces-actor"
    "plexspaces-behavior"
    "plexspaces-mailbox"
    "plexspaces-facet"
    "plexspaces-node"
    "plexspaces-journaling"
    "plexspaces-tuplespace"
    "plexspaces-wasm-runtime"
    "plexspaces-actor-service"
    "plexspaces-scheduler"
    "plexspaces-grpc-middleware"
    "plexspaces-object-registry"
    "plexspaces-process-groups"
    "plexspaces-locks"
    "plexspaces-channel"
    "plexspaces-circuit-breaker"
    "plexspaces-elastic-pool"
    "plexspaces-keyvalue"
    "plexspaces-supervisor"
    "plexspaces-workflow"
    "plexspaces-firecracker"
    "plexspaces-persistence"
    "plexspaces-lattice"
    "plexspaces-tuplespace-service"
    "plexspaces-cli"
)

echo -e "${BLUE}Running coverage analysis for ${#CRATES[@]} crates...${NC}"
echo

# Summary file
SUMMARY_FILE="target/coverage/coverage-summary.txt"
echo "Coverage Baseline Report - $(date)" > "$SUMMARY_FILE"
echo "========================================" >> "$SUMMARY_FILE"
echo "" >> "$SUMMARY_FILE"

# Track results
TOTAL_CRATES=0
PASSING_CRATES=0
FAILING_CRATES=0

for crate in "${CRATES[@]}"; do
    echo -e "${BLUE}Analyzing: $crate${NC}"
    
    # Run coverage for this crate (exclude generated code)
    if cargo llvm-cov --lib -p "$crate" --all-features \
        --ignore-filename-regex '(proto/src/generated|target/)' \
        --summary-only --lcov --output-path "target/coverage/reports/${crate}.lcov" 2>&1 | tee "/tmp/${crate}-coverage.log"; then
        # Extract coverage percentage from log
        COVERAGE=$(grep -E "^\s*Total\s+\|" "/tmp/${crate}-coverage.log" | awk '{print $3}' | head -1 || echo "N/A")
        
        if [ "$COVERAGE" != "N/A" ] && [ -n "$COVERAGE" ]; then
            echo -e "${GREEN}  ✓ Coverage: $COVERAGE${NC}"
            echo "$crate: $COVERAGE" >> "$SUMMARY_FILE"
            PASSING_CRATES=$((PASSING_CRATES + 1))
        else
            echo -e "${YELLOW}  ⚠️  Coverage: Unable to extract${NC}"
            echo "$crate: Unable to extract" >> "$SUMMARY_FILE"
        fi
        TOTAL_CRATES=$((TOTAL_CRATES + 1))
    else
        echo -e "${RED}  ✗ Coverage analysis failed${NC}"
        echo "$crate: FAILED" >> "$SUMMARY_FILE"
        FAILING_CRATES=$((FAILING_CRATES + 1))
        TOTAL_CRATES=$((TOTAL_CRATES + 1))
    fi
    
    echo
done

# Generate workspace-wide coverage (exclude generated code)
echo -e "${BLUE}Generating workspace-wide coverage report...${NC}"
cargo llvm-cov --workspace --all-features \
    --ignore-filename-regex '(proto/src/generated|target/)' \
    --lcov --output-path target/coverage/coverage.lcov
cargo llvm-cov --workspace --all-features \
    --ignore-filename-regex '(proto/src/generated|target/)' \
    --html --output-dir target/coverage/html

# Append workspace summary
echo "" >> "$SUMMARY_FILE"
echo "Workspace Summary:" >> "$SUMMARY_FILE"
cargo llvm-cov --workspace --all-features \
    --ignore-filename-regex '(proto/src/generated|target/)' \
    --summary-only >> "$SUMMARY_FILE" 2>&1 || true

echo
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}✅ Coverage Baseline Complete!${NC}"
echo
echo -e "${BLUE}Summary:${NC}"
echo "  Total crates analyzed: $TOTAL_CRATES"
echo "  Successful: $PASSING_CRATES"
echo "  Failed: $FAILING_CRATES"
echo
echo -e "${BLUE}Reports:${NC}"
echo "  Summary: $SUMMARY_FILE"
echo "  HTML: target/coverage/html/index.html"
echo "  LCOV: target/coverage/coverage.lcov"
echo
echo -e "${BLUE}Note:${NC} Generated code (proto files) excluded from coverage"
echo

