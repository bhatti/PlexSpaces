#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

GREEN='\033[0;32m'; RED='\033[0;31m'; BOLD='\033[1m'; NC='\033[0m'

echo -e "${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
echo -e "${BOLD}  Job Queue — Flow-Controlled Messaging (Embedded Rust Test)${NC}"
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

PASS=0
TOTAL=9

if echo "$OUTPUT" | grep -q "All benchmark assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "Single-run assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "Key patterns demonstrated"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "Partitioned relay assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "UC1 Thundering herd assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "UC2 Config wave deploy assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "UC3 Heartbeat coalescing assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "UC4 Telemetry fallback assertions passed"; then
    PASS=$((PASS + 1))
fi

if echo "$OUTPUT" | grep -q "UC5 Reliable offline delivery assertions passed"; then
    PASS=$((PASS + 1))
fi

if [ "$PASS" -ge "$TOTAL" ]; then
    echo ""
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
    echo -e "${GREEN}${BOLD}  Job Queue (Rust Embedded) — All tests passed ($PASS/$TOTAL checks)${NC}"
    echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════${NC}"
else
    echo -e "${RED}FAIL: Only $PASS/$TOTAL checks passed${NC}"
    exit 1
fi
