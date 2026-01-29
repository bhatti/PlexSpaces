#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

# Test Docker build and deployment

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
ROOT_DIR="$(cd "$PROJECT_DIR/../.." && pwd)"

echo "=========================================="
echo "PlexSpaces Order Processing - Docker Test"
echo "=========================================="
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

print_section() {
    echo -e "${YELLOW}>>> $1${NC}"
    echo ""
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
    echo ""
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
    echo ""
}

cd "$ROOT_DIR"

# 1. Build Docker image
print_section "Building Docker image..."
if docker build -f examples/order-processing/Dockerfile \
    -t plexspaces/order-processing:latest . ; then
    print_success "Docker build succeeded"
else
    print_error "Docker build failed"
    exit 1
fi

# 2. Run container for smoke test
print_section "Running container smoke test..."
if timeout 10s docker run --rm plexspaces/order-processing:latest || [ $? -eq 124 ]; then
    print_success "Container runs successfully (timed out after 10s as expected)"
else
    print_error "Container failed to run"
    exit 1
fi

# 3. Test docker-compose
print_section "Testing docker-compose configuration..."
cd "$PROJECT_DIR"

if docker-compose config > /dev/null 2>&1; then
    print_success "docker-compose.yml is valid"
else
    print_error "docker-compose.yml validation failed"
    exit 1
fi

# 4. Test docker-compose up (detached)
print_section "Starting services with docker-compose..."
if docker-compose up -d; then
    print_success "Services started"
else
    print_error "Failed to start services"
    docker-compose down
    exit 1
fi

# 5. Wait for services to be healthy
print_section "Waiting for services to be healthy..."
sleep 5

# 6. Check service health
if docker-compose ps | grep -q "Up"; then
    print_success "Services are running"
else
    print_error "Services are not running"
    docker-compose logs
    docker-compose down
    exit 1
fi

# 7. View logs
print_section "Service logs (last 20 lines):"
docker-compose logs --tail=20 order-coordinator

# 8. Cleanup
print_section "Stopping services..."
if docker-compose down; then
    print_success "Services stopped"
else
    print_error "Failed to stop services"
    exit 1
fi

echo "=========================================="
echo -e "${GREEN}Docker tests passed successfully!${NC}"
echo "=========================================="
echo ""
echo "Summary:"
echo "  ✓ Docker image builds"
echo "  ✓ Container runs"
echo "  ✓ docker-compose.yml is valid"
echo "  ✓ Services start and run"
echo "  ✓ Services stop cleanly"
echo ""
