#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

echo "=== Running CSP Channels Tests ==="
echo ""

GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}Running all tests...${NC}"
cargo test --tests

echo ""
echo -e "${GREEN}✅ All tests passed!${NC}"
