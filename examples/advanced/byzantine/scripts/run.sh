#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

# Run Byzantine Generals example with metrics

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "╔════════════════════════════════════════════════════════════╗"
echo "║     Byzantine Generals - Consensus Example                ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

cd "$PROJECT_DIR"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Build
echo -e "${BLUE}Building byzantine example...${NC}"
cargo build --release --bin byzantine
echo -e "${GREEN}✓ Build complete${NC}"
echo ""

# Run
echo -e "${BLUE}Running Byzantine Generals consensus...${NC}"
echo ""
cargo run --release --bin byzantine 2>&1 | tee /tmp/byzantine_output.txt

echo ""
echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗"
echo "║                    Example Complete                        ║"
echo -e "╚════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Extract and display metrics
if grep -q "Metrics:" /tmp/byzantine_output.txt; then
    echo -e "${YELLOW}Key Metrics:${NC}"
    grep -A 3 "Metrics:" /tmp/byzantine_output.txt | sed 's/^/  /'
    echo ""
fi

