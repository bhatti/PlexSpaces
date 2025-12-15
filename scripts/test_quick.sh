#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Quick Test Runner for PlexSpaces
#
# Runs fast subset of tests for rapid development feedback.
# Suitable for pre-commit checks and CI fast path.
#
# Usage:
#   ./scripts/test_quick.sh

set -euo pipefail

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}Running quick tests (unit tests only)...${NC}"
echo ""

# Get project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_ROOT"

# Run unit tests only (no integration or E2E)
cargo test --lib --workspace --no-fail-fast

echo ""
echo -e "${GREEN}✅ Quick tests passed!${NC}"
