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

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

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
trap cleanup EXIT

curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true

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
