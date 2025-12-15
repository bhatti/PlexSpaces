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

PID_DIR="/tmp/wasm-calculator"

echo -e "${BLUE}=== Stopping WASM Calculator Multi-Node Cluster ===${NC}"
echo ""

# Stop nodes in reverse order (coordinator first)
for node in coordinator node2 node3; do
    if [ -f "$PID_DIR/$node.pid" ]; then
        PID=$(cat "$PID_DIR/$node.pid")
        if ps -p $PID > /dev/null 2>&1; then
            echo -e "${BLUE}Stopping $node (PID: $PID)...${NC}"
            kill -SIGTERM $PID

            # Wait for graceful shutdown (max 10 seconds)
            for i in {1..10}; do
                if ! ps -p $PID > /dev/null 2>&1; then
                    echo -e "${GREEN}  ✓ $node stopped gracefully${NC}"
                    break
                fi
                sleep 1
            done

            # Force kill if still running
            if ps -p $PID > /dev/null 2>&1; then
                echo -e "${YELLOW}  Forcing $node to stop...${NC}"
                kill -SIGKILL $PID
                sleep 1
            fi
        else
            echo -e "${YELLOW}$node not running (stale PID file)${NC}"
        fi
        rm -f "$PID_DIR/$node.pid"
    else
        echo -e "${YELLOW}$node PID file not found${NC}"
    fi
done

echo ""
echo -e "${GREEN}✅ All nodes stopped${NC}"
