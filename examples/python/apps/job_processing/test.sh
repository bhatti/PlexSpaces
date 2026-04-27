#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Job Processing - Distributed task processing with TupleSpace

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/job_processing_actor.wasm"
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="job-processing-test"
ACTOR_TYPE="$APP_ID"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Job Processing - TupleSpace Scatter/Gather             ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "${YELLOW}💡 Real-world use case: Distributed task processing (image resize, ETL, ML inference)${NC}"
echo ""

source ~/venv/bin/activate

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
echo "Step 2: Deploy job processing actor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed job processor${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function - send POST with call (request-reply)
# Use /ask for operations that expect a response
send_post() {
    local desc="$1"
    local payload="$2"
    
    # Use /ask for request-reply (ask pattern)
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE/ask" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
        # Extract and show relevant response data
        if echo "$RESPONSE" | grep -q '"job_id"'; then
            JOB_ID=$(echo "$RESPONSE" | grep -o '"job_id":"[^"]*"' | head -1 | cut -d'"' -f4)
            echo "     job_id: $JOB_ID"
        fi
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

echo "Step 3: Submit a job with multiple tasks (Scatter)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Scenario: Image resize job with 3 images${NC}"
send_post "Submit image resize job" '{"msg_type":"submit","payload":{"job_type":"image_resize","tasks":[{"url":"img1.jpg","width":800},{"url":"img2.jpg","width":600},{"url":"img3.jpg","width":400}]}}'
echo ""

echo "Step 4: Worker claims a task (ts_take)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Worker 1 claims task" '{"msg_type":"claim_task","payload":{"job_id":"job-1"}}'
send_post "Worker 2 claims task" '{"msg_type":"claim_task","payload":{"job_id":"job-1"}}'
echo ""

echo "Step 5: Workers submit results (ts_write)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Worker 1 submits result" '{"msg_type":"submit_result","payload":{"job_id":"job-1","task_id":"task-0","result_data":"{\"resized\":true,\"size\":1024}"}}'
send_post "Worker 2 submits result" '{"msg_type":"submit_result","payload":{"job_id":"job-1","task_id":"task-1","result_data":"{\"resized\":true,\"size\":2048}"}}'
echo ""

echo "Step 6: Gather results (ts_read_all)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Gather all results" '{"msg_type":"gather_results","payload":{"job_id":"job-1"}}'
echo ""

echo "Step 7: Check job status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Job status" '{"msg_type":"job_status","payload":{"job_id":"job-1"}}'
send_post "List all jobs" '{"msg_type":"list_jobs","payload":{}}'
echo ""

echo -e "${GREEN}✅ Job Processing Test Complete${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} Scatter: ts_write for distributing tasks"
echo -e "  ${GREEN}•${NC} Atomic task claiming: ts_take (prevents duplicate processing)"
echo -e "  ${GREEN}•${NC} Result submission: ts_write for results"
echo -e "  ${GREEN}•${NC} Gather: ts_read_all for collecting all results"
echo ""
echo -e "${YELLOW}💡 TupleSpace enables decoupled, scalable, fault-tolerant job processing${NC}"
echo ""
