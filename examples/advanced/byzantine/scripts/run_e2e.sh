#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

# Run E2E tests with detailed metrics output

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Byzantine Generals - E2E Tests with Metrics           ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

cd "$PROJECT_DIR"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}Running E2E tests with detailed metrics...${NC}"
echo ""

# Run E2E tests with output
if cargo test --test e2e_application -- --nocapture --test-threads=1 2>&1 | tee /tmp/byzantine_e2e_output.txt; then
    echo ""
    echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗"
    echo "║                    E2E Tests Complete                        ║"
    echo -e "╚════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    # Display metrics if available
    if grep -q "Metrics:" /tmp/byzantine_e2e_output.txt; then
        echo -e "${YELLOW}Key Metrics:${NC}"
        grep -A 5 "Metrics:" /tmp/byzantine_e2e_output.txt | sed 's/^/  /'
        echo ""
    fi
    
    echo -e "${YELLOW}Metrics to Review:${NC}"
    echo "  - Coordination vs Compute Time"
    echo "  - Granularity Ratio (should be > 1.0)"
    echo "  - Consensus Rounds"
    echo "  - Message Count"
    echo ""
else
    echo ""
    echo -e "${RED}E2E tests failed${NC}"
    exit 1
fi

