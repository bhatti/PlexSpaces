#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test multi-node deployment locally (3 nodes in separate terminals)
#
# Usage:
#   ./scripts/test-local-nodes.sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== PlexSpaces Order Processing - Multi-Node Test ===${NC}"
echo ""
echo "This script helps you test the multi-node deployment locally."
echo "You'll need to open 3 terminal windows/tabs."
echo ""

# Build first
echo -e "${YELLOW}Building order-coordinator binary...${NC}"
cargo build --bin order-coordinator

echo -e "${GREEN}Build complete!${NC}"
echo ""
echo -e "${BLUE}=== Setup Instructions ===${NC}"
echo ""
echo "Open 3 terminal windows and run these commands:"
echo ""
echo -e "${YELLOW}Terminal 1 - Node 1 (Orders + Payment):${NC}"
echo "cd $(pwd)"
echo "NODE_ID=node1 NODE_ADDRESS=localhost:9001 SPAWN_SERVICES=orders,payment cargo run --bin order-coordinator"
echo ""
echo -e "${YELLOW}Terminal 2 - Node 2 (Inventory):${NC}"
echo "cd $(pwd)"
echo "NODE_ID=node2 NODE_ADDRESS=localhost:9002 SPAWN_SERVICES=inventory cargo run --bin order-coordinator"
echo ""
echo -e "${YELLOW}Terminal 3 - Node 3 (Shipping):${NC}"
echo "cd $(pwd)"
echo "NODE_ID=node3 NODE_ADDRESS=localhost:9003 SPAWN_SERVICES=shipping cargo run --bin order-coordinator"
echo ""
echo -e "${BLUE}=== Expected Behavior ===${NC}"
echo ""
echo "Node 1:"
echo "  - Runs OrderProcessor + PaymentService"
echo "  - Runs full demo workflow"
echo "  - Discovers and communicates with Node 2 (inventory) and Node 3 (shipping) via gRPC"
echo ""
echo "Node 2:"
echo "  - Runs InventoryService only"
echo "  - Waits for requests from Node 1"
echo "  - Press Ctrl+C to exit"
echo ""
echo "Node 3:"
echo "  - Runs ShippingService only"
echo "  - Waits for requests from Node 1"
echo "  - Press Ctrl+C to exit"
echo ""
echo -e "${BLUE}=== Verification ===${NC}"
echo ""
echo "You should see:"
echo "  ✓ All 3 nodes start and register with ActorRegistry"
echo "  ✓ Node 1 processes full order workflow"
echo "  ✓ Node 2 receives inventory reservation requests"
echo "  ✓ Node 3 receives shipping creation requests"
echo "  ✓ Order completes successfully across all nodes"
echo ""
echo -e "${GREEN}Ready to test! Open 3 terminals and run the commands above.${NC}"
