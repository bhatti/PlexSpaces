#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Undeploy leader-election from PlexSpaces node.
# Usage: ./undeploy.sh [HTTP_PORT]
set -euo pipefail
HTTP_PORT="${1:-8091}"
APP_ID="leader-election"

echo "Undeploying ${APP_ID} from port ${HTTP_PORT}..."
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    "http://localhost:${HTTP_PORT}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
case "$HTTP_CODE" in
    200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
    *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
esac
