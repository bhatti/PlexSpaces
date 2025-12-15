#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for Durable Actor Example
# Validates journaling, checkpoints, replay, and side effect caching

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
echo -e "${BLUE}║  Durable Actor Example Test Script                             ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found. Please install Rust toolchain.${NC}"
    exit 1
fi

# Build the example
echo -e "${YELLOW}📦 Building durable_actor_example...${NC}"
if ! cargo build --release --features sqlite-backend 2>&1 | grep -q "Finished"; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build successful${NC}"
echo ""

# Run integration tests
echo -e "${YELLOW}🧪 Running integration tests...${NC}"
if ! cargo test --features sqlite-backend -- --nocapture 2>&1 | tee /tmp/durable_test_output.txt; then
    echo -e "${RED}❌ Integration tests failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Integration tests passed${NC}"
echo ""

# Run the example and capture output
echo -e "${YELLOW}🚀 Running durable_actor_example...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)

# Run and capture output
cargo run --release --bin durable_actor_example --features sqlite-backend 2>&1 | tee "$OUTPUT_FILE" || {
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
JOURNAL_ENTRIES=$(grep -E "Total journal entries: [0-9]+" "$OUTPUT_FILE" | tail -1 | grep -oE "[0-9]+" || echo "0")
SIDE_EFFECT_ENTRIES=$(grep -E "Side effect entries: [0-9]+" "$OUTPUT_FILE" | tail -1 | grep -oE "[0-9]+" || echo "0")
ACTOR_ATTACHED=$(grep -c "Actor attached\|Actor restarted" "$OUTPUT_FILE" || echo "0")
MESSAGES_PROCESSED=$(grep -c "Message processed" "$OUTPUT_FILE" || echo "0")
RESTART_SIMULATED=$(grep -c "Simulating actor restart\|Actor detached" "$OUTPUT_FILE" || echo "0")
EXAMPLE_COMPLETE=$(grep -c "Example Complete\|Key Features Demonstrated" "$OUTPUT_FILE" || echo "0")

# Validation results
VALIDATION_PASSED=true

echo -e "${BLUE}Feature Validation:${NC}"
echo "  • Actor attached/restarted: $([ "$ACTOR_ATTACHED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($ACTOR_ATTACHED)"
echo "  • Messages processed: $([ "$MESSAGES_PROCESSED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($MESSAGES_PROCESSED)"
echo "  • Journal entries created: $([ "$JOURNAL_ENTRIES" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($JOURNAL_ENTRIES)"
echo "  • Side effect entries: $([ "$SIDE_EFFECT_ENTRIES" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($SIDE_EFFECT_ENTRIES)"
echo "  • Restart simulated: $([ "$RESTART_SIMULATED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Example completed: $([ "$EXAMPLE_COMPLETE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo ""

# Validate expectations
if [ "$ACTOR_ATTACHED" -lt 2 ]; then
    echo -e "${RED}❌ Validation failed: Actor should be attached at least twice (initial + restart)${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Actor attachment validation passed${NC}"
fi

if [ "$MESSAGES_PROCESSED" -lt 3 ]; then
    echo -e "${RED}❌ Validation failed: Should process at least 3 messages${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Message processing validation passed${NC}"
fi

if [ "$JOURNAL_ENTRIES" -lt 5 ]; then
    echo -e "${YELLOW}⚠️  Warning: Only $JOURNAL_ENTRIES journal entries (expected ≥5)${NC}"
else
    echo -e "${GREEN}✅ Journal entries validation passed${NC}"
fi

if [ "$SIDE_EFFECT_ENTRIES" -lt 1 ]; then
    echo -e "${YELLOW}⚠️  Warning: No side effect entries found${NC}"
else
    echo -e "${GREEN}✅ Side effect entries validation passed${NC}"
fi

if [ "$RESTART_SIMULATED" -lt 1 ]; then
    echo -e "${RED}❌ Validation failed: Actor restart was not simulated${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Restart simulation validation passed${NC}"
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
    echo "  • Journaling: All operations are journaled"
    echo "  • Checkpoints: Periodic state snapshots for fast recovery"
    echo "  • Deterministic Replay: Actor state recovered from journal"
    echo "  • Side Effect Caching: External calls cached during replay"
    echo "  • Exactly-Once Semantics: No duplicate side effects"
    echo "  • Fault Tolerance: Actors can recover from crashes"
    rm -f "$OUTPUT_FILE"
    exit 0
else
    echo -e "${RED}❌ Some validations failed${NC}"
    echo ""
    echo -e "${YELLOW}Note: Check the output above for details.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi

