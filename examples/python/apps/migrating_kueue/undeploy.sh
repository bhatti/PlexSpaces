#!/usr/bin/env bash
# Undeploy migrating-kueue apps from PlexSpaces node.
# Usage: ./undeploy.sh [HTTP_PORT] [RUN_ID]
set -euo pipefail
HTTP_PORT="${1:-8091}"
RUN_ID="${2:-$(date +%s)}"
APP_ID="migrating-kueue-scheduler-py-${RUN_ID}"

echo "Undeploying ${APP_ID} from port ${HTTP_PORT}..."
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    "http://localhost:${HTTP_PORT}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
case "$HTTP_CODE" in
    200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
    *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
esac
