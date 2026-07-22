#!/usr/bin/env bash
# Test SkyPilot multi-cloud scheduler (Rust WASM). GenServer: submit_task, get_best_resources, get_status.
# Usage: ./test.sh [HTTP_PORT]
#
# Shell: do not pipe JSON into `python3 - <<'PY'` — the heredoc wins stdin. Use env vars instead.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/scheduler_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="migrating-skypilot-scheduler-rust"
ACTOR_TYPE="scheduler"
INSTANCE_ID="default"

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

echo "Step 0: Build WASM (ensure latest with metrics)"
"$SCRIPT_DIR/build.sh" || { echo -e "\033[0;31mBuild failed\033[0m"; exit 1; }
echo ""

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'


echo "================================================================"
echo "  SkyPilot → PlexSpaces: Multi-cloud ML scheduler (Rust WASM)"
echo "================================================================"

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
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
  -F "name=migrating-skypilot-scheduler-rust" \
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
# submit_task may return error if no resources — that is expected behavior, not a crash
if isinstance(p, dict) and 'error' in p and 'allocation' not in p and 'queue_size' not in p and 'No resources' not in p.get('error',''):
    sys.exit(1)
" 2>/dev/null; then
    echo -e "  ${GREEN}✓ $desc${NC}"
  else
    echo -e "${RED}✗ $desc failed: $response${NC}"
    exit 1
  fi
}

echo ""
echo "Step 2: get_best_resources (CPU-only task, should match gcp:n1-standard-4)"
R2=$(ask_actor "${ACTOR_TYPE}:${INSTANCE_ID}" 15 \
  '{"op":"get_best_resources","task":{"task_id":"query-cpu","task_type":"cpu","gpu_required":false,"gpu_memory_gb":0,"cpu_cores":2,"memory_gb":4}}')
assert_actor_ok "get_best_resources" "$R2"
echo "$R2" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
alloc = p.get('allocation', {})
print('  cloud={} instance={} cost_per_hour={}'.format(
    alloc.get('cloud_provider','?'), alloc.get('instance_type','?'), alloc.get('cost_per_hour','?')))
" 2>/dev/null || true

echo ""
echo "Step 3: submit_task (GPU task)"
R3=$(ask_actor "${ACTOR_TYPE}:${INSTANCE_ID}" 15 \
  '{"op":"submit_task","task":{"task_id":"train-job-1","task_type":"training","gpu_required":true,"gpu_memory_gb":16,"cpu_cores":4,"memory_gb":16}}')
assert_actor_ok "submit_task" "$R3"
echo "$R3" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
alloc = p.get('allocation', {})
err = p.get('error','')
if alloc:
    print('  scheduled: cloud={} instance={}'.format(alloc.get('cloud_provider','?'), alloc.get('instance_type','?')))
else:
    print('  result: {}'.format(err or p))
" 2>/dev/null || true

echo ""
echo "Step 4: get_status"
R4=$(ask_actor "${ACTOR_TYPE}:${INSTANCE_ID}" 10 '{"op":"get_status"}')
assert_actor_ok "get_status" "$R4"
echo "$R4" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  tasks_scheduled={} running={} queue_size={}'.format(
    p.get('tasks_scheduled','?'), p.get('running','?'), p.get('queue_size','?')))
" 2>/dev/null || true

echo ""
echo "================================================================"
echo -e "  ${GREEN}SkyPilot scheduler (Rust) test complete${NC}"
echo "================================================================"
