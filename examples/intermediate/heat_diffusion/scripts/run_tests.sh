#!/bin/bash
# Run all Heat Diffusion tests with clear output

set -e

echo "========================================="
echo "Heat Diffusion - Test Suite"
echo "========================================="
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Unit tests
echo -e "${BLUE}📦 Running Unit Tests...${NC}"
cargo test --lib --quiet
echo -e "${GREEN}✓ Unit tests passed${NC}"
echo ""

# Integration tests
echo -e "${BLUE}🔧 Running Integration Tests...${NC}"
cargo test --test basic_test --quiet
echo -e "${GREEN}✓ Integration tests passed${NC}"
echo ""

# Distributed tests
echo -e "${BLUE}🌐 Running Distributed E2E Tests...${NC}"
cargo test --test distributed_test --quiet
echo -e "${GREEN}✓ Distributed tests passed${NC}"
echo ""

# E2E tests with metrics
echo -e "${BLUE}📊 Running E2E Tests with Detailed Metrics...${NC}"
cargo test --test e2e_test -- --nocapture
echo -e "${GREEN}✓ E2E tests passed${NC}"
echo ""

# Doc tests
echo -e "${BLUE}📚 Running Documentation Tests...${NC}"
cargo test --doc --quiet
echo -e "${GREEN}✓ Doc tests passed${NC}"
echo ""

echo "========================================="
echo -e "${GREEN}✅ All tests passed!${NC}"
echo "========================================="
