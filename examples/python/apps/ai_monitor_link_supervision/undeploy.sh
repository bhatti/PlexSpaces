#!/usr/bin/env bash
# Undeploy py-ai-monitor-link-supervision from PlexSpaces nodes.
# Usage: ./undeploy.sh [node1:port1 node2:port2 ...]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
APP_ID="py-ai-monitor-link-supervision"
NODES="${*:-localhost:8091 localhost:8094}"

# Auto-generate JWT if AUTH_HEADER not already set
if [ -z "${AUTH_HEADER:-}" ] && [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)"
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
if [ -z "${AUTH_HEADER:-}" ] && [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

for node in $NODES; do
    host="${node%%:*}"
    port="${node##*:}"
    echo "Undeploying ${APP_ID} from ${host}:${port}..."
    HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        "http://${host}:${port}/api/v1/applications/${APP_ID}" 2>/dev/null) || HTTP_CODE="000"
    case "$HTTP_CODE" in
        200|204|404) echo "OK (HTTP ${HTTP_CODE})" ;;
        *) echo "WARNING: unexpected HTTP ${HTTP_CODE}" ;;
    esac
done
