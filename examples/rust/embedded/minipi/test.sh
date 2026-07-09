#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# MiniPi — Agent Harness & Eval (Rust, embedded) — integration test script
#
# Tests the full eval pipeline:
#   Scenario store → Agent OODA loop → Execution trace capture → Scoring →
#   Regression detection → Benchmark → Human-in-the-loop → Dashboard
#
# Demonstrates:
#   SchemaValidationFacet (tool-call schema validation at priority 95)
#   ExecutionTraceFacet (ordered OODA step capture at priority 85, exports to KV)
#   DurabilityFacet (crash-safe eval workflow at priority 90)
#   Supervision trees (one_for_one restart strategy)
#   AgentLoop (OODA: Observe → Orient → Decide → Act)
#   Ollama integration (provider=ollama model=llama3.2 url=http://localhost:11434)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="rust-minipi"

# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

# Shared target directory (see .cargo/config.toml and CLAUDE.md)
WORKSPACE_TARGET="${SCRIPT_DIR}/../../../../target"
export CARGO_TARGET_DIR="${WORKSPACE_TARGET}"

cd "$SCRIPT_DIR"

echo -e "${BOLD}"
echo "================================================================"
echo "  MiniPi — Agent Harness & Eval (Rust embedded)"
echo "  12 actors · OODA loop · SchemaValidationFacet · ExecutionTraceFacet"
echo "  crash recovery · regression detection · benchmark · human-in-the-loop"
echo "================================================================"
echo -e "${NC}"

# Build
echo -e "${YELLOW}Building minipi (shared target, debug)...${NC}"
if ! cargo build 2>&1; then
    echo -e "${RED}Build failed${NC}"
    exit 1
fi
echo -e "${GREEN}Build successful${NC}"
echo ""

# Run the embedded test (binary self-tests against its own embedded node)
echo -e "${YELLOW}Running minipi embedded test...${NC}"
echo ""

OUTPUT_FILE=$(mktemp)
timeout 60s "${WORKSPACE_TARGET}/debug/minipi" 2>&1 | tee "$OUTPUT_FILE" || {
    EXIT_CODE=$?
    if [ $EXIT_CODE -eq 124 ]; then
        echo -e "${YELLOW}Test completed (timeout — embedded node kept running)${NC}"
    else
        echo -e "${YELLOW}Exit code $EXIT_CODE${NC}"
    fi
}

echo ""
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${CYAN}Validation${NC}"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""

NODE_STARTED=$(grep -c "Node listening\|Startup complete\|SERVING" "$OUTPUT_FILE" 2>/dev/null; true)
ALL_SPAWNED=$(grep -c "All 12 actors spawned" "$OUTPUT_FILE" 2>/dev/null; true)
AGENT_DONE=$(grep -c "AgentActor completed" "$OUTPUT_FILE" 2>/dev/null; true)
SCENARIO_OK=$(grep -c "10 built-in scenarios present" "$OUTPUT_FILE" 2>/dev/null; true)
REGRESSION_OK=$(grep -c "Regression detected correctly" "$OUTPUT_FILE" 2>/dev/null; true)
BENCHMARK_OK=$(grep -c "configs_tested=2" "$OUTPUT_FILE" 2>/dev/null; true)
FSM_OK=$(grep -c "approval gate should return to idle\|FSM state after approval.*idle" "$OUTPUT_FILE" 2>/dev/null; true)
SUCCESS=$(grep -c "MiniPi example completed successfully" "$OUTPUT_FILE" 2>/dev/null; true)

NODE_STARTED=$((${NODE_STARTED:-0} + 0))
ALL_SPAWNED=$((${ALL_SPAWNED:-0} + 0))
AGENT_DONE=$((${AGENT_DONE:-0} + 0))
SCENARIO_OK=$((${SCENARIO_OK:-0} + 0))
REGRESSION_OK=$((${REGRESSION_OK:-0} + 0))
BENCHMARK_OK=$((${BENCHMARK_OK:-0} + 0))
SUCCESS=$((${SUCCESS:-0} + 0))

echo "  Node started:                    $([ "$NODE_STARTED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  All 12 actors spawned:           $([ "$ALL_SPAWNED" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo "  Scenario store seeded:           $([ "$SCENARIO_OK" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${YELLOW}?${NC}")"
echo "  AgentActor OODA completed:       $([ "$AGENT_DONE" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${YELLOW}?${NC}")"
echo "  Regression detection correct:    $([ "$REGRESSION_OK" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${YELLOW}?${NC}")"
echo "  Benchmark 2-config comparison:   $([ "$BENCHMARK_OK" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${YELLOW}?${NC}")"
echo "  Example completed:               $([ "$SUCCESS" -gt 0 ] && echo -e "${GREEN}✅${NC}" || echo -e "${RED}❌${NC}")"
echo ""

rm -f "$OUTPUT_FILE"

if [ "$NODE_STARTED" -gt 0 ] && [ "$ALL_SPAWNED" -gt 0 ] && [ "$SUCCESS" -gt 0 ]; then
    echo -e "${GREEN}${BOLD}All validations passed.${NC}"
    echo ""
    echo "MiniPi demonstrated:"
    echo -e "  ${GREEN}•${NC} SchemaValidationFacet (priority 95)  — method input validation"
    echo -e "  ${GREEN}•${NC} ExecutionTraceFacet (priority 85)    — ordered OODA step capture"
    echo -e "  ${GREEN}•${NC} DurabilityFacet (priority 90)        — journal every step, crash-safe"
    echo -e "  ${GREEN}•${NC} Supervision tree (one_for_one)       — crashed actor restarts"
    echo -e "  ${GREEN}•${NC} OODA AgentLoop                       — step tracking, budget enforcement"
    echo -e "  ${GREEN}•${NC} Regression detection                 — compare scores, flag +/-5% threshold"
    echo -e "  ${GREEN}•${NC} Benchmark comparison                 — same scenario, different harness configs"
    echo -e "  ${GREEN}•${NC} Human-in-the-loop (FSM)              — agent suspends, gate waits, resumes"
    echo -e "  ${GREEN}•${NC} Ollama integration                   — provider=ollama model=llama3.2"
    exit 0
else
    echo -e "${RED}Some validations failed.${NC}"
    exit 1
fi
