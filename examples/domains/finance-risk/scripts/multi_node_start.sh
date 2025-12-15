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

echo -e "${BLUE}=== Starting Finance Risk Assessment Multi-Node Cluster ===${NC}"
echo ""

cd "$PROJECT_DIR"

# Check required environment variables
if [ -z "$SENDGRID_API_KEY" ]; then
    echo -e "${YELLOW}Warning: SENDGRID_API_KEY not set (email notifications disabled)${NC}"
fi

# Create PID directory
mkdir -p /tmp/finance-risk
PID_DIR="/tmp/finance-risk"

# Check if already running
if [ -f "$PID_DIR/coordinator.pid" ]; then
    if ps -p $(cat "$PID_DIR/coordinator.pid") > /dev/null 2>&1; then
        echo -e "${RED}Error: Cluster appears to be already running${NC}"
        echo "Run ./scripts/multi_node_stop.sh first"
        exit 1
    fi
fi

# Build binaries
echo -e "${BLUE}Building finance-node binary...${NC}"
cargo build --release --bin finance-node

FINANCE_NODE="../../target/release/finance-node"

if [ ! -f "$FINANCE_NODE" ]; then
    echo -e "${RED}Error: finance-node binary not found${NC}"
    echo "Expected: $FINANCE_NODE"
    exit 1
fi

# Create data directories
mkdir -p data/finance
mkdir -p logs

# Start Node 4 first (post-decision services - leaf dependency)
echo -e "${BLUE}Starting Node 4 (Post-Decision Services)...${NC}"
RUST_LOG=info $FINANCE_NODE --config config/node4.toml \
    > logs/node4.log 2>&1 &
echo $! > "$PID_DIR/node4.pid"
echo -e "${GREEN}  Node 4 started (PID: $(cat $PID_DIR/node4.pid))${NC}"
sleep 2

# Start Node 3 (risk scoring worker)
echo -e "${BLUE}Starting Node 3 (Risk Scoring Worker)...${NC}"
RUST_LOG=info $FINANCE_NODE --config config/node3.toml \
    > logs/node3.log 2>&1 &
echo $! > "$PID_DIR/node3.pid"
echo -e "${GREEN}  Node 3 started (PID: $(cat $PID_DIR/node3.pid))${NC}"
sleep 2

# Start Node 2 (data collection workers)
echo -e "${BLUE}Starting Node 2 (Data Collection Workers)...${NC}"
RUST_LOG=info $FINANCE_NODE --config config/node2.toml \
    > logs/node2.log 2>&1 &
echo $! > "$PID_DIR/node2.pid"
echo -e "${GREEN}  Node 2 started (PID: $(cat $PID_DIR/node2.pid))${NC}"
sleep 2

# Start Node 1 (coordinator - must be last)
echo -e "${BLUE}Starting Node 1 (Coordinator)...${NC}"
RUST_LOG=info $FINANCE_NODE --config config/node1.toml \
    > logs/node1.log 2>&1 &
echo $! > "$PID_DIR/coordinator.pid"
echo -e "${GREEN}  Node 1 started (PID: $(cat $PID_DIR/coordinator.pid))${NC}"
sleep 3

# Verify all nodes are running
echo ""
echo -e "${GREEN}=== Cluster Status ===${NC}"
ALL_RUNNING=true

for node in coordinator node2 node3 node4; do
    if [ -f "$PID_DIR/$node.pid" ]; then
        PID=$(cat "$PID_DIR/$node.pid")
        if ps -p $PID > /dev/null 2>&1; then
            echo -e "  ${GREEN}✓${NC} $node (PID: $PID)"
        else
            echo -e "  ${RED}✗${NC} $node (failed to start)"
            ALL_RUNNING=false
        fi
    fi
done

if [ "$ALL_RUNNING" = false ]; then
    echo ""
    echo -e "${RED}Some nodes failed to start. Check logs:${NC}"
    echo "  tail -f logs/node*.log"
    exit 1
fi

echo ""
echo -e "${GREEN}=== Node Endpoints ===${NC}"
echo "  Coordinator:      localhost:9101 (gRPC)"
echo "  Data Collection:  localhost:9102 (gRPC)"
echo "  Risk Scoring:     localhost:9103 (gRPC)"
echo "  Post-Decision:    localhost:9104 (gRPC)"

echo ""
echo -e "${GREEN}=== Useful Commands ===${NC}"
echo "  View logs:       tail -f logs/node1.log"
echo "  View all logs:   tail -f logs/*.log"
echo "  Stop cluster:    ./scripts/multi_node_stop.sh"
echo "  Check status:    ps aux | grep finance-node"

echo ""
echo -e "${YELLOW}Note: Actual CLI commands pending PlexSpaces node implementation${NC}"
echo -e "${GREEN}✅ Multi-node cluster is running!${NC}"
