#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Weather Actor (Python WASM): get_weather, cache_stats, clear_cache.
# Usage:
#   ./test.sh [HTTP_PORT]     # contract tests + full example run
#   ./test.sh --contract-only # contract tests only
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SDK_DIR="$PROJECT_ROOT/sdks/python"
WASM_FILE="$SCRIPT_DIR/weather_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="weather-python-test"
RUN_ID="$(date +%s)"

if [[ "${1:-}" == "--contract-only" ]]; then
  HTTP_PORT=""
fi

source "$PROJECT_ROOT/examples/rust/apps/test-common.sh"
source "$HOME/venv/bin/activate" 2>/dev/null || true

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

echo "================================================================"
echo "  Weather Actor (Python WASM, service link + KV cache)"
echo "================================================================"

echo "Step 0: Contract tests"
PYTHONPATH="$SDK_DIR:$SCRIPT_DIR" python3 "$SCRIPT_DIR/test_weather_actor.py"
echo -e "  ${GREEN}Contract tests passed${NC}"
echo ""

if [[ -z "$HTTP_PORT" ]]; then
  echo -e "${GREEN}Contract-only run complete${NC}"
  exit 0
fi

echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""


HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 2: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_ID" \
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
echo -e "  ${GREEN}Weather Actor (Python) example test complete${NC}"
echo "================================================================"
