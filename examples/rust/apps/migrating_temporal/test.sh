#!/usr/bin/env bash
# Test Order Fulfillment Workflow - Rust WASM
# Usage: ./test.sh [HTTP_PORT]
#
# Shell: pass JSON to Python via env (e.g. STATS_RESP=...), not `echo | python3 - <<'PY'`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/order_fulfillment_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="temporal-order-fulfillment-rust"
ACTOR_TYPE="order-fulfillment"
NUM_ORDERS=50
NODE_ID="${PLEXSPACES_NODE_ID:-test-node-${HTTP_PORT}}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"



# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    echo "Output was: $JWT_OUTPUT"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

echo "================================================================"
echo "  Order Fulfillment Workflow (Rust WASM)"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
fi

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node. Start: ./scripts/server.sh${NC}"
  exit 1
fi

echo "Step 1: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
  -F "application_id=$APP_ID" \
  -F "name=temporal-order-fulfillment-rust" \
  -F "version=1.0.0" \
  -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed $APP_ID${NC}"
sleep 1

ask_op() {
  local instance_id="$1" payload="$2" timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:${HTTP_PORT}/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${instance_id}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

assert_actor_ok() {
  local desc="$1" response="$2"
  if echo "$response" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
if isinstance(p, dict) and 'error' in p and 'order_id' not in p and 'status' not in p:
    sys.exit(1)
" 2>/dev/null; then
    echo -e "  ${GREEN}✓ $desc${NC}"
  else
    echo -e "${RED}✗ $desc failed: $response${NC}"
    exit 1
  fi
}

echo ""
echo "Step 2: workflow_run (order-1)"
R2=$(ask_op "order-1" '{"op":"workflow_run","order_id":"order-1","items":["item-a","item-b"],"amount_cents":4999}' 30)
assert_actor_ok "workflow_run order-1" "$R2"
echo "$R2" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  order_id={} status={}'.format(p.get('order_id','?'), p.get('status','?')))
" 2>/dev/null || true

echo ""
echo "Step 3: workflow_query:status"
R3=$(ask_op "order-1" '{"op":"workflow_query:status"}' 10)
assert_actor_ok "workflow_query:status" "$R3"
echo "$R3" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  status={} steps_completed={}'.format(p.get('status','?'), len(p.get('steps',[]))))
" 2>/dev/null || true

APP_LIST=$(fetch_applications_list_json "http://localhost:$HTTP_PORT")
export APP_LIST
APP_LIST="$APP_LIST" APP_ID="$APP_ID" NODE_ID="$NODE_ID" python3 - <<'PY' || true
import json, os, sys, urllib.request
raw = os.environ.get("APP_LIST", "[]")
app_id = os.environ.get("APP_ID", "")
node_id = os.environ.get("NODE_ID", "")
try:
    data = json.loads(raw)
except json.JSONDecodeError:
    print("  (Could not parse GET /api/v1/applications)")
    sys.exit(0)
apps = data.get("applications", data) if isinstance(data, dict) else data
if not isinstance(apps, list):
    apps = []
found = None
for a in apps:
    if not isinstance(a, dict):
        continue
    aid = a.get("application_id") or a.get("name") or ""
    if aid == app_id or app_id in str(aid):
        found = a
        break
if not found:
    print()
    print("  Application metrics (HTTP): app not found in list (ok if API shape differs)")
    sys.exit(0)
m = found.get("metrics") or (found.get("application") or {}).get("metrics")
if m is None:
    print()
    print("  Application metrics (HTTP): no metrics field on list entry")
    sys.exit(0)
cm = m.get("counter_metrics") or {}
print()
print("  Application metrics (merged via host::application_metrics_add)")
print("  ────────────────────────────────────────────")
for k in sorted(cm.keys()):
    if k.startswith("workflow"):
        print(f"  {k}: {cm[k]}")
lat = m.get("latency_totals_ms") or {}
if lat:
    print("  latency_totals_ms (workflow.*):")
    for k in sorted(lat.keys()):
        if "workflow" in k:
            print(f"    {k}: {lat[k]}")
PY

echo ""
echo "================================================================"
echo -e "  ${GREEN}Order Fulfillment Workflow (Rust) Test Complete${NC}"
echo "================================================================"
