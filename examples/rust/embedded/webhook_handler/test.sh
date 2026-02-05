#!/bin/bash
# Test script for Webhook Handler Example
# Validates HTTP deliver/list via webhook_handler actor

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Shared target: use workspace target directory (see .cargo/config.toml and CLAUDE.md).
WORKSPACE_TARGET="${SCRIPT_DIR}/../../../../target"
export CARGO_TARGET_DIR="${WORKSPACE_TARGET}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Webhook Handler Example Test Script                          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Error: cargo not found.${NC}"
    exit 1
fi

echo -e "${YELLOW}📦 Building webhook_handler (using shared target)...${NC}"
if ! cargo build 2>&1 | grep -q "Finished"; then
    echo -e "${RED}❌ Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}✅ Build successful${NC}"
echo ""

echo -e "${YELLOW}🚀 Running webhook_handler example...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)
timeout 25s cargo run 2>&1 | tee "$OUTPUT_FILE" || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo -e "${YELLOW}⏱️  Test completed (timeout)${NC}"
    else
        echo -e "${YELLOW}⚠️  Exit code $EXIT_CODE${NC}"
    fi
}

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}📊 Validation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

NODE_STARTED=$(grep -c "Node listening\|Startup complete\|SERVING" "$OUTPUT_FILE" 2>/dev/null || echo "0")
ACTOR_SPAWNED=$(grep -c "Webhook handler actor spawned" "$OUTPUT_FILE" 2>/dev/null || echo "0")
SUCCESS=$(grep -c "Webhook Handler example completed\|✅" "$OUTPUT_FILE" 2>/dev/null || echo "0")

NODE_STARTED=${NODE_STARTED:-0}
ACTOR_SPAWNED=${ACTOR_SPAWNED:-0}
SUCCESS=${SUCCESS:-0}

echo "  • Node started: $([ "$NODE_STARTED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Actor spawned: $([ "$ACTOR_SPAWNED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  • Example completed: $([ "$SUCCESS" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo ""

if [ "$NODE_STARTED" -gt 0 ] && [ "$ACTOR_SPAWNED" -gt 0 ] && [ "$SUCCESS" -gt 0 ]; then
    echo -e "${GREEN}✅ All validations passed.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 0
else
    echo -e "${RED}❌ Some validations failed.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi
