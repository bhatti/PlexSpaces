#!/usr/bin/env bash
# Undeploy from PlexSpaces node.
# Usage: ./undeploy.sh [HTTP_PORT]
set -euo pipefail
HTTP_PORT="${1:-8091}"
APP_ID="parameter_server-python"

AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

echo "Undeploying ${APP_ID} from port ${HTTP_PORT}..."
if [ -n "$AUTH_HEADER" ]; then
  HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
      "http://localhost:${HTTP_PORT}/api/v1/applications/${APP_ID}" \
      -H "$AUTH_HEADER" 2>/dev/null) || HTTP_CODE="000"
else
  HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
      "http://localhost:${HTTP_PORT}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
fi
case "$HTTP_CODE" in
    200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
    *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
esac
