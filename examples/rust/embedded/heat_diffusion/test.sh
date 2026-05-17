#!/bin/bash
# Test script for Heat Diffusion example (embedded).
# Builds and runs the example; validates metrics output (coord vs compute, granularity).
# Usage:
#   ./test.sh                              # single-node embedded (default)
#   ./test.sh [HTTP_PORT]                   # same as above; port is for convention (embedded runs in-process)
#   ./test.sh "host1:port1 host2:port2 ..." # multi-node: peer list for future use (currently runs single embedded process)
# Uses shared workspace target and debug build.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Optional args: single port (number) -> localhost:port; else all args are host:port list (for multi-node convention)
if [[ -z "${1:-}" ]]; then
  NODES="embedded"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

# Shared target: workspace target directory (see .cargo/config.toml)
WORKSPACE_TARGET="${SCRIPT_DIR}/../../../../target"
export CARGO_TARGET_DIR="${WORKSPACE_TARGET}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "================================================================"
echo "  Heat Diffusion Example Test  nodes=${NODES}"
echo "================================================================"
echo ""

if ! command -v cargo &>/dev/null; then
    echo -e "${RED}Error: cargo not found.${NC}"
    exit 1
fi

echo -e "${YELLOW}Building heat_diffusion (shared target, debug)...${NC}"
cargo build 2>&1 || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo -e "${GREEN}Build OK${NC}"
echo ""

# When NODES is "host:port ..." (multi-node list), pass for future use (embedded binary does not connect to peers yet)
if [[ "$NODES" != "embedded" ]]; then
  export PLEXSPACES_PEERS="${NODES// /,}"
  echo -e "${YELLOW}PLEXSPACES_PEERS=${PLEXSPACES_PEERS} (for future multi-node)${NC}"
fi

echo -e "${YELLOW}Running heat_diffusion...${NC}"
OUTPUT_FILE=$(mktemp)
# Run the already-built binary directly to avoid triggering recompilation inside the timeout.
BINARY="${WORKSPACE_TARGET}/debug/heat_diffusion"
timeout 45s "$BINARY" 2>&1 | tee "$OUTPUT_FILE" || true
echo ""

echo "Validation: checking for benchmarks output..."
if grep -q "BENCHMARKS" "$OUTPUT_FILE" && \
   grep -q "Compute time:" "$OUTPUT_FILE" && \
   grep -q "Coord time:" "$OUTPUT_FILE" && \
   grep -q "Data size:" "$OUTPUT_FILE" && \
   grep -q "Granularity" "$OUTPUT_FILE" && \
   grep -q "Errors:" "$OUTPUT_FILE" && \
   grep -q "Heat Diffusion Example Complete" "$OUTPUT_FILE"; then
    echo -e "${GREEN}Metrics and completion message found.${NC}"
else
    echo -e "${RED}Missing expected metrics or completion message.${NC}"
    rm -f "$OUTPUT_FILE"
    exit 1
fi

rm -f "$OUTPUT_FILE"
echo ""
echo -e "${GREEN}Heat Diffusion test passed.${NC}"
exit 0
