#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Deploy bank_account WASM to running node (HTTP gateway).

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXAMPLE_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
HTTP_PORT="${1:-8091}"
APP_ID="${2:-bank-test-ts}"
WASM_FILE="$EXAMPLE_DIR/account_actor.wasm"
CONFIG_FILE="$EXAMPLE_DIR/app-config.toml"

if [ ! -f "$WASM_FILE" ]; then
  echo "ERROR: WASM not found: $WASM_FILE (run ./scripts/build.sh first)"
  exit 1
fi

echo "Deploying to http://localhost:$HTTP_PORT ..."
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_ID" \
  -F "version=1.0.0" \
  -F "app_file=@$APP_ZIP" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
  echo "✓ Deployed $APP_ID"
else
  echo "Deploy failed: $RESPONSE"
  exit 1
fi
