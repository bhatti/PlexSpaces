#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

GREEN='\033[0;32m'; RED='\033[0;31m'; BOLD='\033[1m'; NC='\033[0m'

echo -e "${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
echo -e "${BOLD}  Metrics Aggregation Pipeline - Embedded Rust Test${NC}"
echo -e "${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
echo ""

echo -e "${BOLD}Step 1: Build${NC}"
cd "$SCRIPT_DIR"
CARGO_TARGET_DIR="${REPO_ROOT}/target" cargo build 2>&1 | tail -3
echo -e "${GREEN}  ✓ Build succeeded${NC}"

echo ""
echo -e "${BOLD}Step 2: Run${NC}"
OUTPUT=$(CARGO_TARGET_DIR="${REPO_ROOT}/target" cargo run 2>&1)
echo "$OUTPUT"

if echo "$OUTPUT" | grep -q "All assertions passed"; then
    echo ""
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}${BOLD}  Metrics Aggregation (Rust Embedded) — All tests passed${NC}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
else
    echo -e "${RED}FAIL: Assertions did not pass${NC}"
    exit 1
fi
