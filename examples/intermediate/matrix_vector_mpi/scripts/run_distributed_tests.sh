#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

set -e

echo "=========================================="
echo "  Matrix-Vector MPI - Distributed Tests"
echo "=========================================="
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Check if running in distributed mode or simulation
MODE=${1:-"simulation"}

if [ "$MODE" == "simulation" ]; then
    echo -e "${BLUE}Running simulated distributed test (single process)...${NC}"
    echo ""
    cargo test --test distributed_test test_distributed_simulation -- --nocapture

    echo ""
    echo -e "${GREEN}✅ Simulated distributed test passed${NC}"
    echo ""
    echo -e "${YELLOW}To run REAL distributed tests across multiple processes:${NC}"
    echo "  ./scripts/run_distributed_tests.sh multi-process"

elif [ "$MODE" == "multi-process" ]; then
    echo -e "${BLUE}Running distributed tests across multiple processes...${NC}"
    echo ""
    echo -e "${YELLOW}INSTRUCTIONS:${NC}"
    echo ""
    echo "This requires running 3 separate terminal windows:"
    echo ""
    echo -e "${GREEN}Terminal 1 (Master):${NC}"
    echo "  cargo test --test distributed_test test_distributed_master -- --ignored --nocapture"
    echo ""
    echo -e "${GREEN}Terminal 2 (Worker 0):${NC}"
    echo "  cargo test --test distributed_test test_distributed_worker_0 -- --ignored --nocapture"
    echo ""
    echo -e "${GREEN}Terminal 3 (Worker 1):${NC}"
    echo "  cargo test --test distributed_test test_distributed_worker_1 -- --ignored --nocapture"
    echo ""
    echo -e "${YELLOW}Execution Order:${NC}"
    echo "  1. Start Master (Terminal 1)"
    echo "  2. Start Worker 0 (Terminal 2)"
    echo "  3. Start Worker 1 (Terminal 3)"
    echo "  4. Watch output in Master terminal"
    echo ""
    echo -e "${RED}NOTE: Make sure ports 9001, 9002, 9003 are available${NC}"

elif [ "$MODE" == "automated" ]; then
    echo -e "${BLUE}Starting automated distributed test...${NC}"
    echo ""

    # Kill any existing processes
    pkill -f "distributed_test" 2>/dev/null || true
    sleep 1

    echo "Starting master node..."
    cargo test --test distributed_test test_distributed_master -- --ignored --nocapture > /tmp/master.log 2>&1 &
    MASTER_PID=$!

    sleep 2

    echo "Starting worker 0..."
    cargo test --test distributed_test test_distributed_worker_0 -- --ignored --nocapture > /tmp/worker0.log 2>&1 &
    WORKER0_PID=$!

    echo "Starting worker 1..."
    cargo test --test distributed_test test_distributed_worker_1 -- --ignored --nocapture > /tmp/worker1.log 2>&1 &
    WORKER1_PID=$!

    echo ""
    echo "Processes started:"
    echo "  Master PID: $MASTER_PID (log: /tmp/master.log)"
    echo "  Worker 0 PID: $WORKER0_PID (log: /tmp/worker0.log)"
    echo "  Worker 1 PID: $WORKER1_PID (log: /tmp/worker1.log)"
    echo ""
    echo "Waiting for tests to complete (15 seconds)..."

    sleep 15

    echo ""
    echo "=== Master Output ==="
    cat /tmp/master.log

    echo ""
    echo "=== Worker 0 Output ==="
    cat /tmp/worker0.log

    echo ""
    echo "=== Worker 1 Output ==="
    cat /tmp/worker1.log

    # Cleanup
    kill $MASTER_PID $WORKER0_PID $WORKER1_PID 2>/dev/null || true

    echo ""
    echo -e "${GREEN}✅ Automated distributed test complete${NC}"

else
    echo -e "${RED}Unknown mode: $MODE${NC}"
    echo "Usage: $0 [simulation|multi-process|automated]"
    exit 1
fi

echo ""
echo -e "${GREEN}=========================================="
echo "  Distributed Tests Complete"
echo -e "==========================================${NC}"
