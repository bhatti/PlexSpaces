#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

echo "=== Running Matrix-Vector MPI Tests ==="
echo ""

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Run all tests
echo -e "${BLUE}Running all tests...${NC}"
cargo test --tests

echo ""
echo -e "${GREEN}✅ All tests passed!${NC}"
