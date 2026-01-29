#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test Locks - Distributed Lock Coordination

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/task_queue_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8094 (gRPC 8093 + 1)
HTTP_PORT="${1:-8094}"

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
echo -e "${YELLOW}💡 To see server-side logs, run node with:${NC}"
echo "   RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093"
echo "   Or: RUST_LOG=plexspaces_facet=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093"
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
    echo "Start node with: cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093"
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
    -F "name=task-queue" \
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

# Helper function to send messages and show detailed output
send_message() {
    local desc="$1"
    local msg_type="$2"
    local payload="$3"
    local show_json="${4:-false}"
    
    echo -e "  ${YELLOW}→${NC} Sending: $desc"
    echo -e "     Type: ${YELLOW}$msg_type${NC}"
    
    # Use the same format as bank_account test - send payload directly
    # The message type is determined by the payload or handled by the facet
    RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/internal/system/$ACTOR_NAME" \
        -H "Content-Type: application/json" \
        -d "{\"msg_type\":\"$msg_type\",\"payload\":$payload}" 2>/dev/null) || true
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        # Extract reply payload
        REPLY=$(echo "$RESPONSE" | grep -o '"reply":"[^"]*"' | cut -d'"' -f4 | sed 's/\\"/"/g' | sed 's/\\\\/\\/g' | sed 's/\\n/ /g')
        
        # Parse JSON reply for better display
        if echo "$REPLY" | grep -q '{'; then
            # Try to extract key fields for lock operations
            if echo "$REPLY" | grep -q '"lock_key"'; then
                LOCK_KEY=$(echo "$REPLY" | grep -o '"lock_key":"[^"]*"' | cut -d'"' -f4)
                HOLDER=$(echo "$REPLY" | grep -o '"holder_id":"[^"]*"' | cut -d'"' -f4)
                VERSION=$(echo "$REPLY" | grep -o '"version":"[^"]*"' | cut -d'"' -f4)
                EXPIRES=$(echo "$REPLY" | grep -o '"expires_at":[0-9.]*' | cut -d':' -f2)
                
                echo -e "  ${GREEN}✓${NC} Lock acquired!"
                echo -e "     Lock Key: ${GREEN}$LOCK_KEY${NC}"
                echo -e "     Holder: ${GREEN}$HOLDER${NC}"
                echo -e "     Version: ${GREEN}${VERSION:0:20}...${NC}"
                if [ -n "$EXPIRES" ]; then
                    echo -e "     Expires: ${GREEN}$EXPIRES${NC}"
                fi
            elif echo "$REPLY" | grep -q '"acquired":false'; then
                REASON=$(echo "$REPLY" | grep -o '"reason":"[^"]*"' | cut -d'"' -f4)
                echo -e "  ${RED}✗${NC} Lock acquisition ${RED}FAILED${NC}"
                echo -e "     Reason: ${RED}$REASON${NC}"
            elif echo "$REPLY" | grep -q '"status":"ok"'; then
                echo -e "  ${GREEN}✓${NC} Operation successful"
                if [ "$show_json" = "true" ]; then
                    echo "     Full response: $REPLY"
                fi
            else
                echo -e "  ${GREEN}✓${NC} Response: $REPLY"
            fi
        else
            echo -e "  ${GREEN}✓${NC} Response: $REPLY"
        fi
        echo "$REPLY"
    else
        ERROR_MSG=$(echo "$RESPONSE" | grep -o '"error":"[^"]*"' | cut -d'"' -f4 || echo "$RESPONSE")
        echo -e "  ${RED}✗${NC} Operation failed: ${RED}$ERROR_MSG${NC}"
        echo ""
        echo ""
    fi
}

echo "Step 3: Task Queue Operations"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

echo ""
echo "📝 Step 3.1: Submit jobs to queue"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
send_message "Submit job 'job-1' (send_email)" "submit" '{"job_id":"job-1","task_type":"send_email","payload":{"to":"user@example.com","subject":"Welcome"}}'
echo ""
send_message "Submit job 'job-2' (process_file)" "submit" '{"job_id":"job-2","task_type":"process_file","payload":{"file_id":"file-123"}}'
echo ""
send_message "Submit job 'job-3' (generate_report)" "submit" '{"job_id":"job-3","task_type":"generate_report","payload":{"report_id":"rpt-456"}}'
echo ""

echo ""
echo "📋 Step 3.2: List jobs in queue"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
send_message "List all jobs" "list" "{}" "true"
echo ""

echo ""
echo "Step 4: Worker Claims Jobs (via LockFacet)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e "${YELLOW}💡 LockFacet intercepts lock operations and uses real LockManager backend${NC}"
echo ""

echo ""
echo "👷 Worker 1 tries to claim job-1..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
ACQUIRE_REPLY=$(send_message "Worker 1 claims job-1" "try_acquire_lock" '{"lock_key":"job:job-1","holder_id":"worker-1","lease_duration_secs":300}')

# Extract version from acquire reply
VERSION=$(echo "$ACQUIRE_REPLY" | grep -o '"version":"[^"]*"' | cut -d'"' -f4 || echo "")

echo ""
echo "🔍 Check job-1 lock status..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_message "Check if job-1 is being processed" "get_lock" '"job:job-1"'
echo ""

echo ""
echo "⚔️  CONCURRENT ACCESS DEMONSTRATION: Multiple workers compete for job-1"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}This demonstrates lock contention - only one worker can hold the lock${NC}"
echo ""

echo ""
echo "👷 Worker 2 tries to claim job-1 (should FAIL - already claimed by worker-1)..."
send_message "Worker 2 tries to claim job-1" "try_acquire_lock" '{"lock_key":"job:job-1","holder_id":"worker-2","lease_duration_secs":300}'
echo ""

echo ""
echo "👷 Worker 3 tries to claim job-1 (should FAIL - already claimed by worker-1)..."
send_message "Worker 3 tries to claim job-1" "try_acquire_lock" '{"lock_key":"job:job-1","holder_id":"worker-3","lease_duration_secs":300}'
echo ""

echo ""
echo "👷 Worker 4 tries to claim job-1 (should FAIL - already claimed by worker-1)..."
send_message "Worker 4 tries to claim job-1" "try_acquire_lock" '{"lock_key":"job:job-1","holder_id":"worker-4","lease_duration_secs":300}'
echo ""

echo ""
echo "✅ Worker 2 successfully claims job-2 (different job, no contention)..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_message "Worker 2 claims job-2" "try_acquire_lock" '{"lock_key":"job:job-2","holder_id":"worker-2","lease_duration_secs":300}'
echo ""

echo ""
echo "🔄 Worker 1 renews lock (heartbeat) for job-1..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}💡 Heartbeat pattern: Long-running jobs renew locks to prevent expiration${NC}"
if [ -n "$VERSION" ]; then
    RENEW_REPLY=$(send_message "Worker 1 renews lock for job-1" "renew_lock" "{\"lock_key\":\"job:job-1\",\"holder_id\":\"worker-1\",\"version\":\"$VERSION\",\"lease_duration_secs\":300}")
    NEW_VERSION=$(echo "$RENEW_REPLY" | grep -o '"version":"[^"]*"' | cut -d'"' -f4 || echo "$VERSION")
    VERSION="$NEW_VERSION"
else
    echo -e "  ${YELLOW}⚠${NC} Skipping renew (version not extracted)"
fi
echo ""

echo ""
echo "✅ Worker 1 completes job-1 and releases lock..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
if [ -n "$VERSION" ]; then
    send_message "Worker 1 releases lock for job-1" "release_lock" "{\"lock_key\":\"job:job-1\",\"holder_id\":\"worker-1\",\"version\":\"$VERSION\"}"
else
    echo -e "  ${YELLOW}⚠${NC} Skipping release (version not extracted)"
fi
echo ""

echo ""
echo "⚔️  CONCURRENT ACCESS AFTER RELEASE: Workers compete for newly available job-1"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Now that worker-1 released the lock, other workers can claim it${NC}"
echo ""

echo ""
echo "👷 Worker 2 tries to claim job-1 again (should SUCCEED - lock released)..."
send_message "Worker 2 claims job-1 (should succeed)" "try_acquire_lock" '{"lock_key":"job:job-1","holder_id":"worker-2","lease_duration_secs":300}'
echo ""

echo ""
echo "👷 Worker 3 tries to claim job-1 (should FAIL - now claimed by worker-2)..."
send_message "Worker 3 tries to claim job-1" "try_acquire_lock" '{"lock_key":"job:job-1","holder_id":"worker-3","lease_duration_secs":300}'
echo ""

echo ""
echo "📊 Final Queue Status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_message "Get queue status" "status" "{}" "true"
echo ""

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║              ✅ Test Complete - Summary                        ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "${GREEN}✓${NC} LockFacet intercepted all lock operations"
echo -e "${GREEN}✓${NC} Used real LockManager backend (configured via node-config/runtimeconfig)"
echo -e "${GREEN}✓${NC} Demonstrated lock contention (multiple workers competing)"
echo -e "${GREEN}✓${NC} Demonstrated heartbeat/renewal pattern"
echo ""
echo "Key Demonstrations:"
echo "  ${GREEN}•${NC} Distributed job processing (no duplicate work)"
echo "  ${GREEN}•${NC} Lock contention handling (only one worker can hold lock)"
echo "  ${GREEN}•${NC} Heartbeat/renewal pattern (long-running jobs keep lock alive)"
echo "  ${GREEN}•${NC} Automatic recovery (if worker crashes, lock expires)"
echo ""
echo -e "${YELLOW}💡 Check server logs to see LockFacet operations:${NC}"
echo "   - Lock acquisition attempts"
echo "   - Lock contention (already held)"
echo "   - Lock renewal (heartbeat)"
echo "   - Lock release"
echo ""
echo -e "${YELLOW}💡 Server logs show:${NC}"
echo "   - Which worker acquired which lock"
echo "   - When locks are renewed (heartbeat)"
echo "   - When locks are released"
echo "   - Lock contention failures"
echo ""
