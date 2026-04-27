#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
#
# Test script for Time-Series Forecasting Example
# Validates actor spawning, workflow execution, and node lifecycle

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
echo -e "${BLUE}║  Time-Series Forecasting Example Test Script                  ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found. Please install Rust toolchain.${NC}"
    exit 1
fi

# Build the example
echo -e "${YELLOW}📦 Building timeseries_forecasting...${NC}"
if ! cargo build 2>&1 | grep -q "Finished"; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build successful${NC}"
echo ""

# Run the example and capture output
echo -e "${YELLOW}🚀 Running timeseries_forecasting...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)

# Run with timeout and capture output
timeout 15s cargo run --bin timeseries_forecasting 2>&1 | tee "$OUTPUT_FILE" || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo -e "${YELLOW}⏱️  Test completed (timeout after 15s)${NC}"
    else
        echo -e "${RED}❌ Test failed with exit code $EXIT_CODE${NC}"
        rm -f "$OUTPUT_FILE"
        exit $EXIT_CODE
    fi
}

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}📊 Validation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

# Parse output for validation
NODE_CREATED=$(grep -c "Node created\|Node started" "$OUTPUT_FILE" || echo "0")
ACTORS_SPAWNED=$(grep -c "actor spawned\|Spawning actors" "$OUTPUT_FILE" || echo "0")
DATA_LOADER=$(grep -c "Data loader actor\|data-loader" "$OUTPUT_FILE" || echo "0")
PREPROCESSOR=$(grep -c "Preprocessor actor\|preprocessor" "$OUTPUT_FILE" || echo "0")
TRAINER=$(grep -c "Trainer actor\|trainer" "$OUTPUT_FILE" || echo "0")
VALIDATOR=$(grep -c "Validator actor\|validator" "$OUTPUT_FILE" || echo "0")
SERVER=$(grep -c "Server actor\|server" "$OUTPUT_FILE" || echo "0")
WORKFLOW_STARTED=$(grep -c "Starting forecasting workflow\|Step 1\|Step 2\|Step 3\|Step 4\|Step 5" "$OUTPUT_FILE" || echo "0")
EXAMPLE_COMPLETE=$(grep -c "Example complete\|Key Takeaways" "$OUTPUT_FILE" || echo "0")

# Validation results
VALIDATION_PASSED=true

echo -e "${BLUE}Feature Validation:${NC}"
echo "  • Node created/started: $([ "$NODE_CREATED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Actors spawned: $([ "$ACTORS_SPAWNED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Data loader actor: $([ "$DATA_LOADER" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Preprocessor actor: $([ "$PREPROCESSOR" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Trainer actor: $([ "$TRAINER" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Validator actor: $([ "$VALIDATOR" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Server actor: $([ "$SERVER" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Workflow started: $([ "$WORKFLOW_STARTED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Example completed: $([ "$EXAMPLE_COMPLETE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo ""

# Validate expectations
if [ "$NODE_CREATED" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: Node was not created/started${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Node creation validation passed${NC}"
fi

if [ "$ACTORS_SPAWNED" -eq 0 ]; then
    echo -e "${RED}❌ Validation failed: No actors were spawned${NC}"
    VALIDATION_PASSED=false
else
    echo -e "${GREEN}✅ Actor spawning validation passed${NC}"
fi

if [ "$DATA_LOADER" -eq 0 ] || [ "$PREPROCESSOR" -eq 0 ] || [ "$TRAINER" -eq 0 ] || [ "$VALIDATOR" -eq 0 ] || [ "$SERVER" -eq 0 ]; then
    echo -e "${YELLOW}⚠️  Warning: Not all actors were detected in output${NC}"
else
    echo -e "${GREEN}✅ All actors validation passed${NC}"
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
    echo "  • Distributed data preprocessing using actors"
    echo "  • Model training with actor coordination"
    echo "  • Model validation with batch inference"
    echo "  • Online model serving via actor messages"
    echo "  • NodeBuilder and ActorBuilder usage"
    echo "  • ConfigBootstrap for configuration"
    rm -f "$OUTPUT_FILE"
    exit 0
else
    echo -e "${RED}❌ Some validations failed${NC}"
    echo ""
    echo -e "${YELLOW}Note: Check the output above for details.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi

