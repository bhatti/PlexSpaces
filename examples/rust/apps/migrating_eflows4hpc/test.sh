#!/usr/bin/env bash
# Test HPC Ensemble (EFlows4HPC-style): coordinator + workers, tuple space + process group.
# Usage: ./test.sh [HTTP_PORT]
#
# Pass JSON to Python via env (METRICS_JSON=...), not `echo | python3 - <<'PY'`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/ensemble_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-eflows4hpc-ensemble"
ACTOR_TYPE="ensemble"
NUM_TASKS=20

GREEN='\033[0;32m'
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
echo "  EFlows4HPC → PlexSpaces: HPC Ensemble (Rust WASM)"
echo "================================================================"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Missing $WASM_FILE after build${NC}"
  exit 1
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
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
  -F "name=migrating-eflows4hpc-ensemble" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1) || true
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

ask_actor() {
  local actor_path="$1" timeout="$2" payload="$3"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:${HTTP_PORT}/api/v1/actors/$APP_ID/$actor_path/ask?timeout=$timeout" \
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
if isinstance(p, dict) and 'error' in p and 'ensemble_id' not in p and 'status' not in p:
    sys.exit(1)
" 2>/dev/null; then
    echo -e "  ${GREEN}✓ $desc${NC}"
  else
    echo -e "${RED}✗ $desc failed: $response${NC}"
    exit 1
  fi
}

echo ""
echo "Step 2: Wake workers (join process group via tasks_ready)"
R_W0=$(ask_actor "${ACTOR_TYPE}:worker-0" 10 '{"op":"tasks_ready","ensemble_id":"","num_tasks":0}')
assert_actor_ok "tasks_ready worker-0" "$R_W0"
R_W1=$(ask_actor "${ACTOR_TYPE}:worker-1" 10 '{"op":"tasks_ready","ensemble_id":"","num_tasks":0}')
assert_actor_ok "tasks_ready worker-1" "$R_W1"

echo ""
echo "Step 3: workflow_run on coordinator (ensemble_id=ens-smoke, num_tasks=$NUM_TASKS)"
R3=$(ask_actor "${ACTOR_TYPE}:coord-1" 45 \
  "{\"op\":\"workflow_run\",\"ensemble_id\":\"ens-smoke\",\"num_tasks\":$NUM_TASKS}")
assert_actor_ok "workflow_run" "$R3"
echo "$R3" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  ensemble_id={} status={} num_completed={}/{}'.format(
    p.get('ensemble_id','?'), p.get('status','?'), p.get('num_completed','?'), p.get('num_tasks','?')))
" 2>/dev/null || true

echo ""
echo "Step 4: workflow_query:status"
R4=$(ask_actor "${ACTOR_TYPE}:coord-1" 10 '{"op":"workflow_query:status"}')
assert_actor_ok "workflow_query:status" "$R4"
echo "$R4" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  status={} compute_ms={} coord_ms={}'.format(
    p.get('status','?'), p.get('total_compute_ms','?'), p.get('total_coord_ms','?')))
" 2>/dev/null || true

echo ""
echo "================================================================"
echo -e "  ${GREEN}HPC Ensemble (EFlows4HPC) Test Complete${NC}"
echo "================================================================"
