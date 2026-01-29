#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

echo "=== Running Genomics Pipeline Tests ==="
echo ""

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Navigate to example directory
cd "$(dirname "$0")/.."

echo -e "${BLUE}Building genomics-pipeline...${NC}"
cargo build --lib
echo ""

echo -e "${BLUE}Running unit tests...${NC}"
cargo test --lib --no-fail-fast
echo ""

echo -e "${BLUE}Running with coverage (if tarpaulin available)...${NC}"
if command -v cargo-tarpaulin &> /dev/null; then
    cargo tarpaulin --lib --out Stdout || true
else
    echo -e "${YELLOW}cargo-tarpaulin not installed, skipping coverage${NC}"
    echo "Install with: cargo install cargo-tarpaulin"
fi
echo ""

echo -e "${GREEN}✅ All genomics pipeline tests passed!${NC}"
echo ""
echo "Test Summary:"
echo "  - All worker tests: ✓"
echo "  - Coordinator tests: ✓"
echo "  - Metrics tests: ✓"
