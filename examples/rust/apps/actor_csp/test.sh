#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/actor_csp.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
export CARGO_PROFILE="${CARGO_PROFILE:-debug}"

# Auto-generate JWT if not provided
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

if [[ -f "$HOME/venv/bin/activate" ]]; then
  # shellcheck disable=SC1090
  source "$HOME/venv/bin/activate"
fi

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES=""
  for _port in "$@"; do
    NODES="${NODES:+$NODES }localhost:$_port"
  done
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="actor-csp-rust"
ORCHESTRATOR_ACTOR="orchestrator"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"

echo "=== Actor-CSP: Structured Concurrency Test ==="
echo "Entry: ${ENTRY_HOST}:${ENTRY_PORT}"
echo ""

# Build if WASM not present
if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  bash "$SCRIPT_DIR/build.sh"
fi

# Undeploy first (ignore errors)
curl -s -X DELETE \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/${APP_ID}" >/dev/null 2>&1 || true
sleep 2

# Deploy
echo "Deploying actor-csp-rust..."
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=actor-csp-rust" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed (HTTP $HTTP_CODE), retrying in 3s..."
  sleep 3
done
rm -f "$APP_ZIP"

if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}FAIL: Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo "  Deploy OK"
sleep 2

# Trigger scatter-gather
echo ""
echo "Triggering scatter-gather (5 services, first 3, 300ms timeout)..."
SCATTER_PAYLOAD='{"op":"scatter_gather","request_id":"test-001","num_services":5,"first_k":3,"timeout_ms":300}'

RESPONSE=$(curl -s \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -H "Content-Type: application/json" \
  -d "$SCATTER_PAYLOAD" \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/${APP_ID}/${ORCHESTRATOR_ACTOR}/ask" 2>/dev/null) || RESPONSE=""

echo "  Response: $RESPONSE"

# Wait for timeout to fire and collect results
sleep 1

echo ""
echo "Checking status..."
STATUS_PAYLOAD='{"op":"status"}'
STATUS_RESPONSE=$(curl -s \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -H "Content-Type: application/json" \
  -d "$STATUS_PAYLOAD" \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/${APP_ID}/${ORCHESTRATOR_ACTOR}/ask" 2>/dev/null) || STATUS_RESPONSE=""

echo "  Status: $STATUS_RESPONSE"

# Validate
if echo "$RESPONSE" | python3 -c "
import sys, json
data = json.load(sys.stdin)
payload = data.get('payload', data)
if isinstance(payload, str):
    payload = json.loads(payload)
assert payload.get('status') == 'scattered', f'expected scattered, got {payload}'
assert payload.get('workers_spawned') == 5, f'expected 5 workers, got {payload}'
print('  Assertions passed')
" 2>/dev/null; then
  echo -e "${GREEN}✅ Actor-CSP test passed${NC}"
else
  echo -e "${RED}❌ Actor-CSP test failed${NC}"
  exit 1
fi

echo ""
echo "Done."
