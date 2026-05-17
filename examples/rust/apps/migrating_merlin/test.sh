#!/usr/bin/env bash
# Test parameter sweep — Merlin style (Rust WASM): pool + tuple space; fallback process group.
# Usage: ./test.sh [HTTP_PORT]
#
# Pass JSON to Python via env (METRICS_JSON=...), not `echo | python3 - <<'PY'`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/sweep_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-merlin-sweep-rust"
ACTOR_TYPE="sweep"
NUM_PARAMS=80
BATCH_SWEEPS=10
BATCH_PARAMS_PER_SWEEP=24
POOL_SIZE_FIRST=2

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"


echo "================================================================"
echo "  Merlin → PlexSpaces: Parameter sweep (worker pool, elastic)"
echo "================================================================"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Missing $WASM_FILE after build${NC}"
  exit 1
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 1: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=migrating-merlin-sweep-rust" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Merlin parameter sweep (Rust) test complete${NC}"
echo "================================================================"
