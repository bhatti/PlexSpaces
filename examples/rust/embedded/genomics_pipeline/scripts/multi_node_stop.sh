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

echo -e "${BLUE}=== Stopping Genomics Pipeline Multi-Node Cluster ===${NC}"

PID_DIR="/tmp/genomics-pipeline"

# Check if cluster is running
if [ ! -d "$PID_DIR" ] || [ ! -f "$PID_DIR/coordinator.pid" ]; then
    echo -e "${YELLOW}No running cluster found${NC}"
    exit 0
fi

# Stop nodes in reverse order (coordinator first, workers last)
stop_node() {
    local node=$1
    local pid_file="$PID_DIR/$node.pid"

    if [ -f "$pid_file" ]; then
        PID=$(cat "$pid_file")
        if ps -p $PID > /dev/null 2>&1; then
            echo -e "${BLUE}Stopping $node (PID: $PID)...${NC}"
            kill -TERM $PID || true

            # Wait for graceful shutdown (max 10 seconds)
            for i in {1..10}; do
                if ! ps -p $PID > /dev/null 2>&1; then
                    echo -e "${GREEN}  ✓ $node stopped${NC}"
                    rm -f "$pid_file"
                    return 0
                fi
                sleep 1
            done

            # Force kill if still running
            if ps -p $PID > /dev/null 2>&1; then
                echo -e "${YELLOW}  Force killing $node...${NC}"
                kill -9 $PID || true
                sleep 1
            fi

            echo -e "${GREEN}  ✓ $node stopped${NC}"
            rm -f "$pid_file"
        else
            echo -e "${YELLOW}  $node not running (stale PID file)${NC}"
            rm -f "$pid_file"
        fi
    fi
}

# Stop in order: coordinator, workers, then leaf nodes
stop_node "coordinator"
sleep 1
stop_node "node2"
stop_node "node3"
stop_node "node4"

# Clean up PID directory
rm -rf "$PID_DIR"

echo ""
echo -e "${GREEN}✅ Multi-node cluster stopped successfully${NC}"
