#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Build and deploy ws_chat_room — leaves the app running for browser use.
#
# Usage:
#   bash deploy.sh [port]
#
# After deploy, open: http://localhost:<port>/apps/ts-ws-chat-room/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/ws_chat_room"
OUTPUT_WASM="$TARGET_DIR/chat_server_actor.wasm"

HTTP_PORT="${1:-${WS_PORT:-8091}}"
HTTP_URL="http://localhost:${HTTP_PORT}"
APP_ID="ts-ws-chat-room"

log() { echo "[deploy.sh] $*"; }
fail() { echo "[FAIL] $*" >&2; exit 1; }

# ─── Build ────────────────────────────────────────────────────────────────────
log "Building ws_chat_room…"
cd "$SCRIPT_DIR"
bash build.sh
[ -f "$OUTPUT_WASM" ] || fail "WASM not built: $OUTPUT_WASM"

# ─── Auth ─────────────────────────────────────────────────────────────────────
export AUTH_HEADER="${AUTH_HEADER:-}"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

# ─── Check node ───────────────────────────────────────────────────────────────
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "${HTTP_URL}/health" 2>/dev/null || echo "000")
[ "$HTTP_CODE" = "200" ] || fail "Node not reachable (HTTP $HTTP_CODE). Start with: bash scripts/server.sh"

# ─── Build zip ────────────────────────────────────────────────────────────────
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "${OUTPUT_WASM}" "${SCRIPT_DIR}/app-config.toml" >/dev/null
(cd "${SCRIPT_DIR}" && zip "$APP_ZIP" static/ static/* 2>/dev/null || true)

# ─── Undeploy previous, then deploy ──────────────────────────────────────────
bash "$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT" 2>/dev/null || true

log "Deploying ${APP_ID}…"
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "${HTTP_URL}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=${APP_ID}" \
    -F "name=${APP_ID}" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  log "Deploy attempt $_attempt failed (HTTP $HTTP_CODE): $RESPONSE"
  sleep 3
done
[ "$_deployed" -eq 1 ] || fail "Deploy failed after 3 attempts: $RESPONSE"

log "Deployed OK"
log ""
log "Open in browser: ${HTTP_URL}/apps/${APP_ID}/"
log "WebSocket URL for the Connect form: ws://localhost:${HTTP_PORT}/ws"
