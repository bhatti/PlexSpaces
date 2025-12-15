#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${BLUE}=== Cleaning up WASM Calculator Data ===${NC}"
echo ""

cd "$PROJECT_DIR"

# Stop nodes if running
if [ -f "/tmp/wasm-calculator/coordinator.pid" ]; then
    echo -e "${YELLOW}Stopping running nodes first...${NC}"
    ./scripts/multi_node_stop.sh
    echo ""
fi

# Remove data directories
if [ -d "data/calculator" ]; then
    echo -e "${BLUE}Removing data directory...${NC}"
    rm -rf data/calculator
    echo -e "${GREEN}  ✓ Removed data/calculator${NC}"
fi

# Remove log files
if [ -d "logs" ] && [ "$(ls -A logs)" ]; then
    echo -e "${BLUE}Removing log files...${NC}"
    rm -f logs/*.log
    echo -e "${GREEN}  ✓ Removed log files${NC}"
fi

# Remove PID directory
if [ -d "/tmp/wasm-calculator" ]; then
    echo -e "${BLUE}Removing PID directory...${NC}"
    rm -rf /tmp/wasm-calculator
    echo -e "${GREEN}  ✓ Removed /tmp/wasm-calculator${NC}"
fi

echo ""
echo -e "${GREEN}✅ Cleanup complete${NC}"
