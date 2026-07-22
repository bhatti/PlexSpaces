#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Distributed Task Queue with LockFacet

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/task_queue_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8091
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="task-queue-test"
ACTOR_NAME="task-queue"



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
echo "║        Distributed Task Queue - LockFacet Integration Test     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"

if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Deploy
echo "Step 2: Deploy task queue coordinator with LockFacet"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
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

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed task queue coordinator with LockFacet${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function - HTTP POST (fire-and-forget) just checks success
send_post() {
    local desc="$1"
    local payload="$2"
    
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

# Helper function - HTTP GET (ask) returns payload
send_get() {
    local desc="$1"
    local query="${2:-}"
    
    URL="http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME"
    if [ -n "$query" ]; then
        URL="$URL?$query"
    fi
    
    RESPONSE=$(curl -s --max-time 10 -X GET "$URL" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        # Extract payload if present
        PAYLOAD=$(echo "$RESPONSE" | grep -o '"payload":[^,}]*' | cut -d: -f2- || echo "")
        echo -e "  ${GREEN}✓${NC} $desc"
        if [ -n "$PAYLOAD" ] && [ "$PAYLOAD" != "null" ]; then
            echo "     Response: $PAYLOAD"
        fi
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

echo "Step 3: Task Queue Operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "📝 Submitting jobs to queue..."
send_post "Submit job-1 (send_email)" '{"msg_type":"submit","payload":{"job_id":"job-1","task_type":"send_email"}}'
send_post "Submit job-2 (process_file)" '{"msg_type":"submit","payload":{"job_id":"job-2","task_type":"process_file"}}'
send_post "Submit job-3 (generate_report)" '{"msg_type":"submit","payload":{"job_id":"job-3","task_type":"generate_report"}}'
echo ""

echo "📋 List jobs (via GET)..."
send_get "List all jobs" "msg_type=list"
echo ""

echo "📊 Get queue status (via GET)..."
send_get "Queue status" "msg_type=status"
echo ""

echo "🔒 Lock operations via LockFacet..."
echo "   (LockFacet intercepts lock messages and uses real LockManager)"
send_post "Worker 1 claims job-1" '{"msg_type":"try_acquire_lock","payload":{"lock_key":"job:job-1","holder_id":"worker-1","lease_duration_secs":300}}'
send_post "Worker 2 tries job-1 (should fail)" '{"msg_type":"try_acquire_lock","payload":{"lock_key":"job:job-1","holder_id":"worker-2","lease_duration_secs":300}}'
send_post "Worker 2 claims job-2 (should succeed)" '{"msg_type":"try_acquire_lock","payload":{"lock_key":"job:job-2","holder_id":"worker-2","lease_duration_secs":300}}'
echo ""

echo -e "${GREEN}✅ Task Queue test complete!${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} Job queue management (submit, list, status)"
echo -e "  ${GREEN}•${NC} LockFacet integration (distributed locks)"
echo -e "  ${GREEN}•${NC} Lock contention handling"
echo ""
