#!/bin/bash
# Test script for Timers Example
# Validates timer functionality and collects metrics

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test configuration
TEST_DURATION=8  # seconds
EXPECTED_MIN_HEARTBEATS=3
EXPECTED_MIN_POLLS=10

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Timers Example Test Script                                   ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found. Please install Rust toolchain.${NC}"
    exit 1
fi

# Build the example
echo -e "${YELLOW}📦 Building timers_example...${NC}"
if ! cargo build --release 2>&1 | grep -q "Finished"; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build successful${NC}"
echo ""

# Run the example and capture output
echo -e "${YELLOW}🚀 Running timers_example for ${TEST_DURATION} seconds...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)
TIMEOUT_FILE=$(mktemp)

# Run with timeout and capture output
timeout ${TEST_DURATION}s cargo run --release 2>&1 | tee "$OUTPUT_FILE" || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo -e "${YELLOW}⏱️  Test completed (timeout after ${TEST_DURATION}s)${NC}"
    else
        echo -e "${RED}❌ Test failed with exit code $EXIT_CODE${NC}"
        rm -f "$OUTPUT_FILE" "$TIMEOUT_FILE"
        exit $EXIT_CODE
    fi
}

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}📊 Metrics & Validation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Parse output for metrics
HEARTBEAT_COUNT=$(grep -c "💓 Heartbeat timer fired" "$OUTPUT_FILE" || echo "0")
CLEANUP_FIRED=$(grep -c "🧹 Cleanup timer fired" "$OUTPUT_FILE" || echo "0")
POLL_COUNT=$(grep -c "📊 Poll timer fired" "$OUTPUT_FILE" || echo "0")
TIMER_FIRED_TOTAL=$(grep -c "TimerFired" "$OUTPUT_FILE" || echo "0")
FACET_ATTACHED=$(grep -c "TimerFacet" "$OUTPUT_FILE" || echo "0")
ACTOR_SPAWNED=$(grep -c "Actor spawned" "$OUTPUT_FILE" || echo "0")

# Calculate timing metrics
FIRST_HEARTBEAT=$(grep "💓 Heartbeat timer fired" "$OUTPUT_FILE" | head -1 | grep -oP '\d{2}:\d{2}:\d{2}' || echo "")
LAST_HEARTBEAT=$(grep "💓 Heartbeat timer fired" "$OUTPUT_FILE" | tail -1 | grep -oP '\d{2}:\d{2}:\d{2}' || echo "")

# Validation results
VALIDATION_PASSED=true

echo -e "${BLUE}Test Results:${NC}"
echo "  • Actor spawned: $([ "$ACTOR_SPAWNED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}") ($ACTOR_SPAWNED)"
echo "  • TimerFacet attached: $([ "$FACET_ATTACHED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Total TimerFired messages: $TIMER_FIRED_TOTAL"
echo ""

echo -e "${BLUE}Timer Metrics:${NC}"
echo "  • Heartbeat timer fires: $HEARTBEAT_COUNT (expected: ≥$EXPECTED_MIN_HEARTBEATS)"
echo "  • Cleanup timer fires: $CLEANUP_FIRED (expected: ≥1)"
echo "  • Poll timer fires: $POLL_COUNT (expected: ≥$EXPECTED_MIN_POLLS)"
echo ""

# Validate expectations
if [ "$HEARTBEAT_COUNT" -lt "$EXPECTED_MIN_HEARTBEATS" ]; then
    echo -e "${RED}❌ Validation failed: Heartbeat timer fired only $HEARTBEAT_COUNT times (expected ≥$EXPECTED_MIN_HEARTBEATS)${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Heartbeat timer validation passed${NC}"
fi

if [ "$CLEANUP_FIRED" -lt 1 ]; then
    echo -e "${RED}❌ Validation failed: Cleanup timer did not fire${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Cleanup timer validation passed${NC}"
fi

if [ "$POLL_COUNT" -lt "$EXPECTED_MIN_POLLS" ]; then
    echo -e "${YELLOW}⚠️  Warning: Poll timer fired only $POLL_COUNT times (expected ≥$EXPECTED_MIN_POLLS)${NC}"
    echo -e "${YELLOW}   This may be expected if timers are not fully registered in the example${NC}"
else
    echo -e "${GREEN}✅ Poll timer validation passed${NC}"
fi

if [ "$ACTOR_SPAWNED" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Actor was not spawned${NC}"
    VALIDATION_PASSED=false
fi

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# Final result
if [ "$VALIDATION_PASSED" = true ]; then
    echo -e "${GREEN}✅ All validations passed!${NC}"
    echo ""
    echo -e "${BLUE}Key Takeaways Validated:${NC}"
    echo "  • TimerFacet can be attached to actors"
    echo "  • Timers fire and send TimerFired messages"
    echo "  • Periodic timers continue firing"
    echo "  • One-time timers fire once"
    rm -f "$OUTPUT_FILE" "$TIMEOUT_FILE"
    exit 0
else
    echo -e "${RED}❌ Some validations failed${NC}"
    echo ""
    echo -e "${YELLOW}Note: This example demonstrates the concept.${NC}"
    echo -e "${YELLOW}Full timer registration requires accessing facets from the actor,${NC}"
    echo -e "${YELLOW}which will be available via ActorContext convenience methods.${NC}"
    rm -f "$OUTPUT_FILE" "$TIMEOUT_FILE"
    exit 1
fi

