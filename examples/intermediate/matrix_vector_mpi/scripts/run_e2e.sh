#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

echo "=========================================="
echo "  Matrix-Vector MPI - E2E Tests"
echo "=========================================="
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}Running E2E tests with detailed metrics...${NC}"
echo ""

# Run E2E tests with output
cargo test --test e2e_test -- --nocapture

echo ""
echo -e "${GREEN}=========================================="
echo "  E2E Tests Complete"
echo -e "==========================================${NC}"
echo ""
echo -e "${YELLOW}Key Metrics to Review:${NC}"
echo "  - Compute vs Coordination Time"
echo "  - Granularity Ratio (should be > 1.0)"
echo "  - Latency Breakdown per Phase"
echo "  - Scaling Characteristics"
