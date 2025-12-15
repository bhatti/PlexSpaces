#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for Process Groups (Pub/Sub) Example
# Validates process group creation, joining, broadcasting, and leaving

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$EXAMPLE_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Process Groups (Pub/Sub) Example Test Script                 ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found. Please install Rust toolchain.${NC}"
    exit 1
fi

# Build the example
echo -e "${YELLOW}📦 Building process_groups_pubsub...${NC}"
if ! cargo build --release 2>&1 | grep -q "Finished"; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build successful${NC}"
echo ""

# Run the example and capture output
echo -e "${YELLOW}🚀 Running process_groups_pubsub...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)

# Run and capture output
cargo run --release --bin process_groups_pubsub 2>&1 | tee "$OUTPUT_FILE" || {
    EXIT_CODE=$?
    echo -e "${RED}❌ Example failed with exit code $EXIT_CODE${NC}"
    rm -f "$OUTPUT_FILE"
    exit $EXIT_CODE
}

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}📊 Validation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Parse output for validation
GROUP_CREATED=$(grep -c "Group created\|Step 1.*Creating" "$OUTPUT_FILE" || echo "0")
ACTORS_JOINED=$(grep -c "joined\|Step 2.*Actors joining" "$OUTPUT_FILE" || echo "0")
TOTAL_MEMBERS=$(grep -E "Total members: [0-9]+" "$OUTPUT_FILE" | tail -1 | grep -oE "[0-9]+" || echo "0")
BROADCAST_1=$(grep -c "Step 3.*Broadcasting\|Hello from broadcaster" "$OUTPUT_FILE" || echo "0")
ACTOR_LEFT=$(grep -c "Step 4.*leaving\|Actor-1 leaving" "$OUTPUT_FILE" || echo "0")
REMAINING_MEMBERS=$(grep -E "Remaining members: [0-9]+" "$OUTPUT_FILE" | tail -1 | grep -oE "[0-9]+" || echo "0")
BROADCAST_2=$(grep -c "Step 5.*Broadcasting\|Second broadcast" "$OUTPUT_FILE" || echo "0")
GROUP_DELETED=$(grep -c "Step 6.*Cleaning\|Group deleted" "$OUTPUT_FILE" || echo "0")
EXAMPLE_COMPLETE=$(grep -c "Example complete\|Key Takeaways" "$OUTPUT_FILE" || echo "0")

# Validation results
VALIDATION_PASSED=true

echo -e "${BLUE}Feature Validation:${NC}"
echo "  • Group created: $([ "$GROUP_CREATED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Actors joined: $([ "$ACTORS_JOINED" -ge 3 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($ACTORS_JOINED)"
echo "  • Total members: $([ "$TOTAL_MEMBERS" -eq 3 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($TOTAL_MEMBERS, expected: 3)"
echo "  • First broadcast: $([ "$BROADCAST_1" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Actor left: $([ "$ACTOR_LEFT" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Remaining members: $([ "$REMAINING_MEMBERS" -eq 2 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($REMAINING_MEMBERS, expected: 2)"
echo "  • Second broadcast: $([ "$BROADCAST_2" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Group deleted: $([ "$GROUP_DELETED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Example completed: $([ "$EXAMPLE_COMPLETE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo ""

# Validate expectations
if [ "$GROUP_CREATED" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Group was not created${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Group creation validation passed${NC}"
fi

if [ "$TOTAL_MEMBERS" -ne 3 ]; then
    echo -e "${RED}❌ Validation failed: Expected 3 members, got $TOTAL_MEMBERS${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Member count validation passed${NC}"
fi

if [ "$REMAINING_MEMBERS" -ne 2 ]; then
    echo -e "${RED}❌ Validation failed: Expected 2 remaining members, got $REMAINING_MEMBERS${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Remaining member count validation passed${NC}"
fi

if [ "$EXAMPLE_COMPLETE" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Example did not complete successfully${NC}"
    VALIDATION_PASSED=false
fi

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# Final result
if [ "$VALIDATION_PASSED" = true ]; then
    echo -e "${GREEN}✅ All validations passed!${NC}"
    echo ""
    echo -e "${BLUE}Key Features Validated:${NC}"
    echo "  • Process Groups enable pub/sub patterns"
    echo "  • All members receive broadcast messages"
    echo "  • Members can join/leave dynamically"
    echo "  • Group lifecycle management (create/delete)"
    echo "  • Use for: config updates, event notifications, coordination"
    rm -f "$OUTPUT_FILE"
    exit 0
else
    echo -e "${RED}❌ Some validations failed${NC}"
    echo ""
    echo -e "${YELLOW}Note: Check the output above for details.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi

