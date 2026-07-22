#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Deploy leader-election app (deploys actor1 and actor2 as children).
# Run once before using compete.sh from two terminals.
# Usage: ./deploy.sh [HTTP_PORT]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/leader_election_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="leader-election"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
fi

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
if echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "${GREEN}Deployed $APP_ID (actor1, actor2)${NC}"
    echo "Run: ./compete.sh actor1 $HTTP_PORT  and  ./compete.sh actor2 $HTTP_PORT"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
