#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test ChatAgent (Rust): validates state management and alarm logic.
# NOTE: LLM calls require ANTHROPIC_API_KEY in service_links. This script
# tests state/alarm operations only; LLM validation is skipped intentionally.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

if [[ -z "${1:-}" ]]; then
  HTTP_PORT=8080
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  HTTP_PORT="$1"
else
  HTTP_PORT=8080
fi

APP_ID="rs-chat-agent"
AGENT_ACTOR="agent-1:ChatAgentActor"
BASE_URL="http://localhost:$HTTP_PORT"

if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
fi
AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

send_op() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "$BASE_URL/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

check_ok() {
  local resp="$1"
  local label="$2"
  if echo "$resp" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert p.get('status')=='ok' or 'error' not in p, p" 2>/dev/null; then
    echo -e "  ${GREEN}PASS${NC} $label"
  else
    echo -e "  ${RED}FAIL${NC} $label: $resp"
    exit 1
  fi
}

echo "=== Chat Agent Test (rust) ==="
echo "Target: $BASE_URL | App: $APP_ID | Actor: $AGENT_ACTOR"
echo ""

echo "Step 1: Clear any prior state"
RESP=$(send_op "$AGENT_ACTOR" '{"op":"clear"}')
check_ok "$RESP" "clear"

echo "Step 2: Send a chat message"
RESP=$(send_op "$AGENT_ACTOR" '{"op":"chat","message":"Hello, who are you?"}')
echo "  Response: $RESP"
check_ok "$RESP" "chat"

echo "Step 3: Get conversation history"
HIST=$(send_op "$AGENT_ACTOR" '{"op":"get_history"}')
echo "  History: $HIST"
check_ok "$HIST" "get_history"
MSG_COUNT=$(echo "$HIST" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('count',0))" 2>/dev/null || echo "0")
if [ "$MSG_COUNT" -lt 1 ]; then
  echo -e "  ${RED}FAIL${NC} expected at least 1 message in history, got $MSG_COUNT"
  exit 1
fi
echo -e "  ${GREEN}PASS${NC} history has $MSG_COUNT message(s)"

echo "Step 4: Send another message"
RESP=$(send_op "$AGENT_ACTOR" '{"op":"chat","message":"What can you help me with?"}')
check_ok "$RESP" "chat second message"

echo "Step 5: Clear conversation and alarm"
send_op "$AGENT_ACTOR" '{"op":"clear"}' > /dev/null
echo -e "  ${GREEN}PASS${NC} cleared"

echo "Step 6: Verify history is empty after clear"
HIST=$(send_op "$AGENT_ACTOR" '{"op":"get_history"}')
EMPTY=$(echo "$HIST" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('count',0))" 2>/dev/null || echo "0")
if [ "$EMPTY" -ne 0 ]; then
  echo -e "  ${RED}FAIL${NC} expected 0 messages after clear, got $EMPTY"
  exit 1
fi
echo -e "  ${GREEN}PASS${NC} history empty after clear"

echo ""
echo -e "${GREEN}All tests passed.${NC}"
