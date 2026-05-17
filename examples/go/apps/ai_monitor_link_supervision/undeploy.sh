#!/usr/bin/env bash
# Undeploy go-ai-monitor-link-supervision from PlexSpaces nodes.
# Usage: ./undeploy.sh [node1:port1 node2:port2 ...]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_ID="go-ai-monitor-link-supervision"
NODES="${*:-localhost:8091 localhost:8094}"
for node in $NODES; do
    host="${node%%:*}"
    port="${node##*:}"
    echo "Undeploying ${APP_ID} from ${host}:${port}..."
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
        "http://${host}:${port}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
    case "$HTTP_CODE" in
        200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
        *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
    esac
done
