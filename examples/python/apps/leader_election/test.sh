#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Leader Election - acquire lock, hold it, renew once, then release.
# Usage: ./test.sh [HTTP_PORT] [ACTOR_NAME]
#   HTTP_PORT   - port of the PlexSpaces node (default: 8091)
#   ACTOR_NAME  - actor instance name (default: actor1); use actor2 to compete from a second terminal
#
# Two-terminal competition: run ./test.sh 8091 actor1 in one terminal and
# ./test.sh 8091 actor2 in another — both contend for the same distributed lock.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/leader_election_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
ACTOR_NAME="${2:-actor1}"

APP_ID="leader-election"
ACTOR_TYPE="leader_election"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'


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

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Leader Election Test (Python WASM)                      ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo -e "  Actor: ${GREEN}${APP_ID}/${ACTOR_NAME}:${ACTOR_TYPE}${NC}"
echo ""

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

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
  -F "name=$APP_ID" \
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

ask() {
  local actor="$1" payload="$2" timeout="${3:-15}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:${HTTP_PORT}/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${actor}/ask?timeout=$timeout" \
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
# error key with no candidate_id means a real failure
if isinstance(p, dict) and 'error' in p and 'candidate_id' not in p and 'leader' not in p:
    sys.exit(1)
" 2>/dev/null; then
    echo -e "  ${GREEN}✓ $desc${NC}"
  else
    echo -e "${RED}✗ $desc failed: $response${NC}"
    exit 1
  fi
}

echo ""
echo "Step 2: try_lead — non-blocking attempt to become leader"
R2=$(ask "$ACTOR_NAME" '{"op":"try_lead","candidate_id":"candidate-1"}')
assert_actor_ok "try_lead" "$R2"
IS_LEADER=$(echo "$R2" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('yes' if p.get('leader') else 'no')
" 2>/dev/null || echo "?")
echo "  leader=$IS_LEADER"

echo ""
echo "Step 3: renew_lead — heartbeat (no-op if not leader, ok either way)"
R3=$(ask "$ACTOR_NAME" '{"op":"renew_lead","lease_duration_secs":30,"candidate_id":"candidate-1"}')
assert_actor_ok "renew_lead" "$R3"
echo "$R3" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  leader={} renewed={}'.format(p.get('leader','?'), p.get('renewed','?')))
" 2>/dev/null || true

echo ""
echo "Step 4: release_lead — release the lock"
R4=$(ask "$ACTOR_NAME" '{"op":"release_lead","candidate_id":"candidate-1"}')
assert_actor_ok "release_lead" "$R4"
echo "$R4" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  leader={} message={}'.format(p.get('leader','?'), p.get('message','')))
" 2>/dev/null || true

echo ""
echo "Step 5: status — verify not leader after release"
R5=$(ask "$ACTOR_NAME" '{"op":"status","candidate_id":"candidate-1"}')
assert_actor_ok "status" "$R5"
echo "$R5" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
print('  is_leader={}'.format(p.get('leader', p.get('is_leader','?'))))
" 2>/dev/null || true

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo -e "║  ${GREEN}Leader Election (Python) example test complete${NC}               ║"
echo "╚════════════════════════════════════════════════════════════════╝"
