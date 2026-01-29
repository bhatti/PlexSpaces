#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo -e "${BLUE}=== Genomics Pipeline Cleanup ===${NC}"
echo ""

cd "$PROJECT_DIR"

# Check if cluster is running
if [ -f "/tmp/genomics-pipeline/coordinator.pid" ]; then
    if ps -p $(cat /tmp/genomics-pipeline/coordinator.pid) > /dev/null 2>&1; then
        echo -e "${RED}Error: Cluster is still running${NC}"
        echo "Stop cluster first:"
        echo "  ./scripts/multi_node_stop.sh   # For multi-node"
        echo "  ./scripts/docker_down.sh       # For Docker"
        exit 1
    fi
fi

# Warn user
echo -e "${YELLOW}This will delete:${NC}"
echo "  - SQLite journal databases (data/genomics/)"
echo "  - Log files (logs/)"
echo "  - PID files (/tmp/genomics-pipeline/)"
echo "  - Docker volumes (if --docker flag used)"
echo ""
read -p "Continue? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo -e "${BLUE}Cleanup cancelled${NC}"
    exit 0
fi

# Clean up local files
if [ -d "data/genomics" ]; then
    echo -e "${BLUE}Removing SQLite databases...${NC}"
    rm -rf data/genomics
    echo -e "${GREEN}  ✓ SQLite databases removed${NC}"
fi

if [ -d "logs" ]; then
    echo -e "${BLUE}Removing log files...${NC}"
    rm -rf logs
    echo -e "${GREEN}  ✓ Log files removed${NC}"
fi

if [ -d "/tmp/genomics-pipeline" ]; then
    echo -e "${BLUE}Removing PID files...${NC}"
    rm -rf /tmp/genomics-pipeline
    echo -e "${GREEN}  ✓ PID files removed${NC}"
fi

# Clean up Docker volumes if requested
if [ "$1" == "--docker" ]; then
    if command -v docker-compose &> /dev/null; then
        echo -e "${BLUE}Removing Docker volumes...${NC}"
        docker-compose down -v 2>/dev/null || true
        echo -e "${GREEN}  ✓ Docker volumes removed${NC}"
    fi
fi

# Clean up build artifacts if requested
if [ "$1" == "--all" ] || [ "$2" == "--all" ]; then
    echo -e "${BLUE}Removing build artifacts...${NC}"
    cargo clean
    echo -e "${GREEN}  ✓ Build artifacts removed${NC}"
fi

echo ""
echo -e "${GREEN}✅ Cleanup complete!${NC}"
echo ""
echo -e "${BLUE}Tip: Use --docker flag to also remove Docker volumes${NC}"
echo -e "${BLUE}Tip: Use --all flag to also remove build artifacts${NC}"
