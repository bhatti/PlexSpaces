#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
set -euo pipefail

HTTP_PORT="${1:-${HTTP_PORT:-8091}}"
HTTP_HOST="${HTTP_HOST:-127.0.0.1}"
APP_ID="rust-job-queue-outbox"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
fi
AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

echo "Undeploying $APP_ID from port $HTTP_PORT..."
RESULT=$(curl -s -w "\n%{http_code}" -X DELETE \
  "http://$HTTP_HOST:$HTTP_PORT/api/v1/applications/$APP_ID" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} 2>&1) || true
HTTP_CODE=$(echo "$RESULT" | tail -n1)
if [ "$HTTP_CODE" = "200" ]; then
  echo "OK (HTTP 200)"
else
  echo "HTTP $HTTP_CODE (may not have been deployed)"
fi
