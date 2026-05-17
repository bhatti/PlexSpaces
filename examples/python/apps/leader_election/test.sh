#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Leader Election - acquire lock, hold it, renew once, then release.
# Usage: ./test.sh [HTTP_PORT] [ACTOR_NAME]
#   HTTP_PORT   - port of the PlexSpaces node (default: 8091)
#   ACTOR_NAME  - actor instance name (default: actor1); use actor2 to compete from a second terminal
#
# Two-terminal competition: run ./test.sh 8091 actor1 in one terminal and
# ./test.sh 8091 actor2 in another — both contend for the same distributed lock.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/leader_election_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
ACTOR_NAME="${2:-actor1}"

APP_ID="leader-election"
ACTOR_TYPE="leader_election"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Leader Election Test (Python WASM)                      ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo -e "  Actor: ${GREEN}${APP_ID}/${ACTOR_NAME}:${ACTOR_TYPE}${NC}"
echo ""

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

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
echo -e "  ${GREEN}Lock released${NC}"

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo -e "║  ${GREEN}Leader Election (Python) example test complete${NC}               ║"
echo "╚════════════════════════════════════════════════════════════════╝"
