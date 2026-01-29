#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Automated validation script for multi-node order-processing deployment
#
# Usage:
#   ./scripts/validate-multinode.sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}PlexSpaces Multi-Node Validation Suite${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Track test results
TESTS_PASSED=0
TESTS_FAILED=0

function pass() {
    echo -e "${GREEN}✓ PASS${NC}: $1"
    ((TESTS_PASSED++))
}

function fail() {
    echo -e "${RED}✗ FAIL${NC}: $1"
    ((TESTS_FAILED++))
}

function info() {
    echo -e "${BLUE}ℹ INFO${NC}: $1"
}

function warn() {
    echo -e "${YELLOW}⚠ WARN${NC}: $1"
}

# Test 1: Verify scripts exist and are executable
echo -e "\n${YELLOW}Test 1: Verify test scripts${NC}"
if [ -x "$SCRIPT_DIR/test-docker-compose.sh" ]; then
    pass "test-docker-compose.sh exists and is executable"
else
    fail "test-docker-compose.sh not found or not executable"
fi

if [ -x "$SCRIPT_DIR/test-local-nodes.sh" ]; then
    pass "test-local-nodes.sh exists and is executable"
else
    fail "test-local-nodes.sh not found or not executable"
fi

# Test 2: Verify Docker Compose file
echo -e "\n${YELLOW}Test 2: Verify Docker Compose configuration${NC}"
if [ -f "$PROJECT_DIR/docker-compose.yml" ]; then
    pass "docker-compose.yml exists"

    # Validate docker-compose syntax
    if docker-compose -f "$PROJECT_DIR/docker-compose.yml" config > /dev/null 2>&1; then
        pass "docker-compose.yml syntax is valid"
    else
        fail "docker-compose.yml has syntax errors"
    fi

    # Check for required services
    if grep -q "node1:" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node1 service defined"
    else
        fail "node1 service not found"
    fi

    if grep -q "node2:" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node2 service defined"
    else
        fail "node2 service not found"
    fi

    if grep -q "node3:" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node3 service defined"
    else
        fail "node3 service not found"
    fi
else
    fail "docker-compose.yml not found"
fi

# Test 3: Verify Dockerfile
echo -e "\n${YELLOW}Test 3: Verify Dockerfile${NC}"
if [ -f "$PROJECT_DIR/Dockerfile" ]; then
    pass "Dockerfile exists"

    # Check for required build stages
    if grep -q "FROM rust:" "$PROJECT_DIR/Dockerfile"; then
        pass "Dockerfile has Rust base image"
    else
        fail "Dockerfile missing Rust base image"
    fi

    if grep -q "COPY Cargo.toml" "$PROJECT_DIR/Dockerfile"; then
        pass "Dockerfile copies Cargo.toml"
    else
        warn "Dockerfile may not copy Cargo.toml properly"
    fi
else
    fail "Dockerfile not found"
fi

# Test 4: Build test (compile order-coordinator)
echo -e "\n${YELLOW}Test 4: Build order-coordinator binary${NC}"
info "Building order-coordinator (this may take a moment)..."
cd "$PROJECT_DIR"
if cargo build --bin order-coordinator --quiet 2>&1 | grep -i error; then
    fail "order-coordinator failed to compile"
else
    pass "order-coordinator compiled successfully"
fi

# Test 5: Check environment variable handling
echo -e "\n${YELLOW}Test 5: Test environment variable parsing${NC}"
info "Testing SPAWN_SERVICES parsing..."

# Create a test that just starts and exits to verify env var parsing
TEST_OUTPUT=$(NODE_ID=test1 NODE_ADDRESS=localhost:9999 SPAWN_SERVICES=payment cargo run --bin order-coordinator 2>&1 &
sleep 2
pkill -f order-coordinator
wait 2>/dev/null || true)

if echo "$TEST_OUTPUT" | grep -q "Enabled services:"; then
    pass "SPAWN_SERVICES environment variable is parsed"
else
    warn "Could not verify SPAWN_SERVICES parsing (process may not have started)"
fi

# Test 6: Docker Compose script commands
echo -e "\n${YELLOW}Test 6: Test docker-compose script commands${NC}"

# Test help command
if "$SCRIPT_DIR/test-docker-compose.sh" help 2>&1 | grep -q "Usage:"; then
    pass "docker-compose script help command works"
else
    fail "docker-compose script help command failed"
fi

# Test 7: Verify node configuration in docker-compose
echo -e "\n${YELLOW}Test 7: Verify node service configurations${NC}"

# Node 1 configuration
if grep -A 5 "node1:" "$PROJECT_DIR/docker-compose.yml" | grep -q "SPAWN_SERVICES=orders,payment"; then
    pass "Node 1 configured with orders+payment services"
else
    fail "Node 1 missing correct SPAWN_SERVICES configuration"
fi

if grep -A 5 "node1:" "$PROJECT_DIR/docker-compose.yml" | grep -q "8000:8000"; then
    pass "Node 1 port mapping correct (8000)"
else
    fail "Node 1 port mapping incorrect"
fi

# Node 2 configuration
if grep -A 5 "node2:" "$PROJECT_DIR/docker-compose.yml" | grep -q "SPAWN_SERVICES=inventory"; then
    pass "Node 2 configured with inventory service"
else
    fail "Node 2 missing correct SPAWN_SERVICES configuration"
fi

# Node 3 configuration
if grep -A 5 "node3:" "$PROJECT_DIR/docker-compose.yml" | grep -q "SPAWN_SERVICES=shipping"; then
    pass "Node 3 configured with shipping service"
else
    fail "Node 3 missing correct SPAWN_SERVICES configuration"
fi

# Test 8: Verify .dockerignore
echo -e "\n${YELLOW}Test 8: Verify Docker build optimization${NC}"
if [ -f "$PROJECT_DIR/.dockerignore" ]; then
    pass ".dockerignore exists"

    if grep -q "target/" "$PROJECT_DIR/.dockerignore"; then
        pass ".dockerignore excludes target/ directory"
    else
        warn ".dockerignore doesn't exclude target/ (builds may be slower)"
    fi
else
    warn ".dockerignore not found (Docker builds may include unnecessary files)"
fi

# Test 9: Check for ActorService integration
echo -e "\n${YELLOW}Test 9: Verify ActorService integration${NC}"
if [ -f "$PROJECT_DIR/src/actor_service.rs" ]; then
    pass "actor_service.rs module exists"

    if grep -q "ActorServiceManager" "$PROJECT_DIR/src/actor_service.rs"; then
        pass "ActorServiceManager implementation found"
    else
        fail "ActorServiceManager not implemented"
    fi

    if grep -q "start_server_with_shutdown" "$PROJECT_DIR/src/actor_service.rs"; then
        pass "Graceful shutdown support implemented"
    else
        warn "Graceful shutdown may not be implemented"
    fi
else
    fail "actor_service.rs module not found"
fi

# Test 10: Verify main.rs integration
echo -e "\n${YELLOW}Test 10: Verify main.rs multi-node setup${NC}"
if [ -f "$PROJECT_DIR/src/main.rs" ]; then
    pass "main.rs exists"

    if grep -q "ActorServiceManager::new" "$PROJECT_DIR/src/main.rs"; then
        pass "main.rs initializes ActorServiceManager"
    else
        fail "main.rs doesn't initialize ActorServiceManager"
    fi

    if grep -q "start_server_with_shutdown" "$PROJECT_DIR/src/main.rs"; then
        pass "main.rs starts gRPC server with graceful shutdown"
    else
        fail "main.rs doesn't start gRPC server properly"
    fi

    if grep -q "register_actor" "$PROJECT_DIR/src/main.rs"; then
        pass "main.rs registers actors with ActorRegistry"
    else
        fail "main.rs doesn't register actors"
    fi
else
    fail "main.rs not found"
fi

# Test 11: Integration test (optional - can be slow)
echo -e "\n${YELLOW}Test 11: Quick smoke test (optional)${NC}"
info "Skipping Docker Compose integration test (run manually with: ./scripts/test-docker-compose.sh up)"
info "To test manually:"
info "  1. Run: cd $PROJECT_DIR && ./scripts/test-docker-compose.sh up"
info "  2. Check logs: ./scripts/test-docker-compose.sh logs"
info "  3. Verify status: ./scripts/test-docker-compose.sh status"
info "  4. Stop: ./scripts/test-docker-compose.sh down"

# Summary
echo -e "\n${BLUE}========================================${NC}"
echo -e "${BLUE}Validation Summary${NC}"
echo -e "${BLUE}========================================${NC}"
echo -e "${GREEN}Tests Passed: $TESTS_PASSED${NC}"
echo -e "${RED}Tests Failed: $TESTS_FAILED${NC}"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All validation tests passed!${NC}"
    echo -e "${GREEN}Multi-node deployment is ready to test.${NC}"
    echo ""
    echo -e "${BLUE}Next steps:${NC}"
    echo "  1. Test with Docker Compose: ./scripts/test-docker-compose.sh up"
    echo "  2. Test locally (3 terminals): ./scripts/test-local-nodes.sh"
    echo "  3. View detailed instructions: cat scripts/test-local-nodes.sh"
    exit 0
else
    echo -e "${RED}✗ Some validation tests failed.${NC}"
    echo -e "${YELLOW}Please fix the issues above before testing multi-node deployment.${NC}"
    exit 1
fi
