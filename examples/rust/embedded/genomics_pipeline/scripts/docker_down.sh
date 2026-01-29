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

echo -e "${BLUE}=== Stopping Genomics Pipeline Docker Cluster ===${NC}"

cd "$PROJECT_DIR"

# Check if cluster is running
if ! docker-compose ps | grep -q "Up"; then
    echo -e "${YELLOW}No running containers found${NC}"
    exit 0
fi

# Show current status
echo -e "${BLUE}Current cluster status:${NC}"
docker-compose ps

# Stop containers
echo ""
echo -e "${BLUE}Stopping containers...${NC}"
docker-compose stop

# Prompt for volume removal
echo ""
read -p "Remove volumes (SQLite data)? [y/N] " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo -e "${YELLOW}Removing volumes...${NC}"
    docker-compose down -v
    echo -e "${RED}⚠️  SQLite journal data has been deleted${NC}"
else
    docker-compose down
    echo -e "${GREEN}✅ Volumes preserved (data persists)${NC}"
fi

echo ""
echo -e "${GREEN}✅ Cluster stopped successfully${NC}"
