#!/usr/bin/env bash
# Undeploy registry-test from PlexSpaces node.
# Usage: ./undeploy.sh [HTTP_PORT]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HTTP_PORT="${1:-8091}"
APP_ID="registry-test"

echo "Undeploying ${APP_ID} from port ${HTTP_PORT}..."
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
    "http://localhost:${HTTP_PORT}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
case "$HTTP_CODE" in
    200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
    *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
esac
