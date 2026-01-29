#!/usr/bin/env bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Quick validation for multi-node setup (no compilation)

# Don't exit on error - we want to show all results
set +e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

TESTS_PASSED=0
TESTS_FAILED=0

function pass() {
    echo -e "${GREEN}✓${NC} $1"
    ((TESTS_PASSED++))
}

function fail() {
    echo -e "${RED}✗${NC} $1"
    ((TESTS_FAILED++))
}

echo -e "${BLUE}Quick Multi-Node Setup Validation${NC}"
echo ""

# Test scripts exist
if [ -x "$SCRIPT_DIR/test-docker-compose.sh" ]; then
    pass "test-docker-compose.sh exists"
else
    fail "test-docker-compose.sh missing"
fi

if [ -x "$SCRIPT_DIR/test-local-nodes.sh" ]; then
    pass "test-local-nodes.sh exists"
else
    fail "test-local-nodes.sh missing"
fi

# Test docker-compose.yml
if [ -f "$PROJECT_DIR/docker-compose.yml" ]; then
    pass "docker-compose.yml exists"

    if grep -q "node1:" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node1 service defined"
    else
        fail "node1 service missing"
    fi

    if grep -q "node2:" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node2 service defined"
    else
        fail "node2 service missing"
    fi

    if grep -q "node3:" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node3 service defined"
    else
        fail "node3 service missing"
    fi

    if grep -q "SPAWN_SERVICES=orders,payment" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node1 config correct"
    else
        fail "node1 config wrong"
    fi

    if grep -q "SPAWN_SERVICES=inventory" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node2 config correct"
    else
        fail "node2 config wrong"
    fi

    if grep -q "SPAWN_SERVICES=shipping" "$PROJECT_DIR/docker-compose.yml"; then
        pass "node3 config correct"
    else
        fail "node3 config wrong"
    fi
else
    fail "docker-compose.yml missing"
fi

# Test Dockerfile
if [ -f "$PROJECT_DIR/Dockerfile" ]; then
    pass "Dockerfile exists"
else
    fail "Dockerfile missing"
fi

# Test actor_service integration
if [ -f "$PROJECT_DIR/src/actor_service.rs" ]; then
    pass "actor_service.rs exists"

    if grep -q "ActorServiceManager" "$PROJECT_DIR/src/actor_service.rs"; then
        pass "ActorServiceManager implemented"
    else
        fail "ActorServiceManager missing"
    fi

    if grep -q "start_server_with_shutdown" "$PROJECT_DIR/src/actor_service.rs"; then
        pass "Graceful shutdown implemented"
    else
        fail "Graceful shutdown missing"
    fi
else
    fail "actor_service.rs missing"
fi

# Test main.rs integration
if [ -f "$PROJECT_DIR/src/main.rs" ]; then
    pass "main.rs exists"

    if grep -q "ActorServiceManager::new" "$PROJECT_DIR/src/main.rs"; then
        pass "ActorServiceManager initialized"
    else
        fail "ActorServiceManager not initialized"
    fi

    if grep -q "register_actor" "$PROJECT_DIR/src/main.rs"; then
        pass "Actor registration implemented"
    else
        fail "Actor registration missing"
    fi
else
    fail "main.rs missing"
fi

echo ""
echo -e "${GREEN}Passed: $TESTS_PASSED${NC} | ${RED}Failed: $TESTS_FAILED${NC}"
echo ""

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All checks passed!${NC}"
    echo ""
    echo "Next steps:"
    echo "  1. Build: cargo build --bin order-coordinator"
    echo "  2. Test with Docker: ./scripts/test-docker-compose.sh up"
    echo "  3. Test locally: ./scripts/test-local-nodes.sh"
    exit 0
else
    echo -e "${RED}✗ Some checks failed${NC}"
    exit 1
fi
