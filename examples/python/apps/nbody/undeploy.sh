#!/usr/bin/env bash
# Undeploy nbody apps from PlexSpaces node.
# Usage: ./undeploy.sh [HTTP_PORT] [NUM_BODIES]
set -euo pipefail
HTTP_PORT="${1:-8091}"
NUM_BODIES="${2:-5}"

for i in $(seq 1 "$NUM_BODIES"); do
    APP_ID="nbody-body-${i}"
    echo "Undeploying ${APP_ID} from port ${HTTP_PORT}..."
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
        "http://localhost:${HTTP_PORT}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
    case "$HTTP_CODE" in
        200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
        *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
    esac
done
