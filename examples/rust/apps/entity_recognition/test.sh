#!/usr/bin/env bash
# Entity recognition (Rust WASM). process_document, batch, get_status, get_metrics.
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/entity_recognition_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="entity-recognition-wasm-rust"
ACTOR_TYPE="entity_extractor"
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

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "\033[0;31mBuild failed\033[0m"; exit 1; }
echo ""

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'


echo "================================================================"
echo "  Entity recognition (Rust WASM, SDK + WIT)"
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
  -F "name=entity-recognition-wasm-rust" \
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
if isinstance(p, dict) and 'error' in p and 'entities' not in p and 'status' not in p:
    sys.exit(1)
" 2>/dev/null; then
    echo -e "  ${GREEN}✓ $desc${NC}"
  else
    echo -e "${RED}✗ $desc failed: $response${NC}"
    exit 1
  fi
}

echo ""
echo "Step 2: process_document (extract entities from text)"
R2=$(ask_actor "${ACTOR_TYPE}:${INSTANCE_ID}" 15 \
  '{"op":"process_document","doc_id":"doc-1","text":"Alice visited New York and met Bob from Google on Monday."}')
assert_actor_ok "process_document" "$R2"
echo "$R2" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
entities = p.get('entities', [])
print('  entities_found={} doc_id={}'.format(len(entities), p.get('doc_id','?')))
" 2>/dev/null || true

echo ""
echo "Step 3: get_status"
R3=$(ask_actor "${ACTOR_TYPE}:${INSTANCE_ID}" 10 '{"op":"get_status"}')
assert_actor_ok "get_status" "$R3"
echo "$R3" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  documents_processed={} total_entities={}'.format(
    p.get('documents_processed', p.get('docs_processed','?')),
    p.get('total_entities', p.get('entities_found','?'))))
" 2>/dev/null || true

echo ""
echo "Step 4: batch (two documents in one call)"
R4=$(ask_actor "${ACTOR_TYPE}:${INSTANCE_ID}" 15 \
  '{"op":"batch","documents":[{"doc_id":"doc-2","text":"Elon Musk founded SpaceX in California."},{"doc_id":"doc-3","text":"The Eiffel Tower is in Paris, France."}]}')
assert_actor_ok "batch" "$R4"
echo "$R4" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
results = p.get('results', [])
print('  batch_results={}'.format(len(results)))
" 2>/dev/null || true

echo ""
echo "================================================================"
echo -e "  ${GREEN}Entity recognition (Rust) test complete${NC}"
echo "================================================================"
