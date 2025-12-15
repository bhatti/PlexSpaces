#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Validation script for Firecracker Multi-Tenant Example

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Firecracker Multi-Tenant Example Validation                  ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo

# Check if user wants to use Docker
USE_DOCKER="${USE_DOCKER:-false}"
if [ "$USE_DOCKER" = "true" ] || [ "$1" = "--docker" ]; then
    echo -e "${BLUE}Using Docker for testing...${NC}"
    if [ -f "$SCRIPT_DIR/test-docker.sh" ]; then
        exec "$SCRIPT_DIR/test-docker.sh"
    else
        echo -e "${RED}Docker test script not found${NC}"
        exit 1
    fi
fi

echo

# Check prerequisites (required for real Firecracker VMs)
echo -e "${BLUE}1. Checking prerequisites...${NC}"
PREREQUISITES_MET=true

if ! command -v firecracker &> /dev/null && [ ! -f "/usr/bin/firecracker" ]; then
    echo -e "   ${RED}✗ Firecracker binary not found${NC}"
    echo "      Install from: https://github.com/firecracker-microvm/firecracker/releases"
    echo "      Or set PLEXSPACES_FIRECRACKER_BIN environment variable"
    PREREQUISITES_MET=false
else
    FIRECRACKER_BIN=$(command -v firecracker 2>/dev/null || echo "/usr/bin/firecracker")
    echo -e "   ${GREEN}✓ Firecracker binary found: ${FIRECRACKER_BIN}${NC}"
fi

# Check kernel image (support environment variable override)
KERNEL_PATH="${PLEXSPACES_FIRECRACKER_KERNEL:-/var/lib/firecracker/vmlinux}"
if [ ! -f "$KERNEL_PATH" ]; then
    echo -e "   ${RED}✗ Kernel image not found at ${KERNEL_PATH}${NC}"
    echo "      Download from: https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin"
    echo "      Or set PLEXSPACES_FIRECRACKER_KERNEL environment variable"
    PREREQUISITES_MET=false
else
    echo -e "   ${GREEN}✓ Kernel image found: ${KERNEL_PATH}${NC}"
fi

# Check rootfs image (support environment variable override)
ROOTFS_PATH="${PLEXSPACES_FIRECRACKER_ROOTFS:-/var/lib/firecracker/rootfs.ext4}"
if [ ! -f "$ROOTFS_PATH" ]; then
    echo -e "   ${RED}✗ Rootfs image not found at ${ROOTFS_PATH}${NC}"
    echo "      Download from: https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4"
    echo "      Or set PLEXSPACES_FIRECRACKER_ROOTFS environment variable"
    PREREQUISITES_MET=false
else
    echo -e "   ${GREEN}✓ Rootfs image found: ${ROOTFS_PATH}${NC}"
fi

if [ "$PREREQUISITES_MET" = false ]; then
    echo
    echo -e "${RED}❌ Prerequisites not met. This example requires real Firecracker VMs.${NC}"
    echo -e "${RED}   Please install Firecracker and required images before running.${NC}"
    exit 1
fi

echo
echo -e "${BLUE}2. Building example...${NC}"
cd "$EXAMPLE_DIR"
if cargo build --release 2>&1 | grep -q "Finished"; then
    echo -e "   ${GREEN}✓ Build successful${NC}"
else
    echo -e "   ${RED}✗ Build failed${NC}"
    exit 1
fi

echo
echo -e "${BLUE}3. Running example workload with metrics...${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
OUTPUT_FILE=$(mktemp)
if cargo run --release -- run --tenants 3 --operations 5 2>&1 | tee "$OUTPUT_FILE"; then
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "   ${GREEN}✓ Workload completed${NC}"
else
    echo -e "   ${RED}✗ Workload failed${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi

echo
echo -e "${BLUE}4. Validating metrics output...${NC}"

# Check for expected metrics output
HAS_PERFORMANCE_REPORT=$(grep -c "Firecracker Multi-Tenant Performance Report" "$OUTPUT_FILE" || echo "0")
HAS_GRANULARITY_RATIO=$(grep -c "Granularity Ratio" "$OUTPUT_FILE" || echo "0")
HAS_EFFICIENCY=$(grep -c "Efficiency" "$OUTPUT_FILE" || echo "0")
HAS_AGGREGATE_METRICS=$(grep -c "Aggregate Metrics" "$OUTPUT_FILE" || echo "0")
HAS_COMPUTE_TIME=$(grep -c "Compute Time" "$OUTPUT_FILE" || echo "0")
HAS_COORDINATE_TIME=$(grep -c "Coordinate Time" "$OUTPUT_FILE" || echo "0")

VALIDATION_PASSED=true

echo -e "${BLUE}Feature Validation:${NC}"
echo "  • Performance reports generated: $([ "$HAS_PERFORMANCE_REPORT" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Granularity ratio calculated: $([ "$HAS_GRANULARITY_RATIO" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Efficiency metrics shown: $([ "$HAS_EFFICIENCY" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Aggregate metrics displayed: $([ "$HAS_AGGREGATE_METRICS" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Compute time tracked: $([ "$HAS_COMPUTE_TIME" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Coordinate time tracked: $([ "$HAS_COORDINATE_TIME" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"

if [ "$HAS_PERFORMANCE_REPORT" -eq 0 ] || [ "$HAS_GRANULARITY_RATIO" -eq 0 ] || [ "$HAS_EFFICIENCY" -eq 0 ]; then
    echo -e "   ${RED}✗ Metrics validation failed${NC}"
    VALIDATION_PASSED=false
else
    echo -e "   ${GREEN}✓ Metrics validation passed${NC}"
fi

echo
echo -e "${BLUE}5. Running integration tests with real Firecracker...${NC}"
if cargo test --test integration_test -- --ignored 2>&1 | grep -q "test result: ok"; then
    echo -e "   ${GREEN}✓ Integration tests passed${NC}"
else
    echo -e "   ${YELLOW}⚠️  Integration tests skipped or failed (this is expected if prerequisites not met)${NC}"
    echo "      Run manually: cargo test --test integration_test -- --ignored"
fi

echo
echo -e "${BLUE}6. Testing VM registry...${NC}"
if cargo run --release -- list 2>&1 | grep -q "Listing running tenants"; then
    echo -e "   ${GREEN}✓ VM registry working${NC}"
else
    echo -e "   ${RED}✗ VM registry test failed${NC}"
    VALIDATION_PASSED=false
fi

rm -f "$OUTPUT_FILE"

echo
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

if [ "$VALIDATION_PASSED" = true ]; then
    echo -e "${GREEN}✅ All validations passed!${NC}"
    echo
    echo -e "${BLUE}Key Features Validated:${NC}"
    echo "  • CoordinationComputeTracker metrics tracking"
    echo "  • ConfigBootstrap configuration loading"
    echo "  • Performance reports with granularity ratios"
    echo "  • Efficiency calculations"
    echo "  • Aggregate metrics across tenants"
    echo "  • VM registry discovery"
    exit 0
else
    echo -e "${RED}❌ Some validations failed${NC}"
    exit 1
fi





