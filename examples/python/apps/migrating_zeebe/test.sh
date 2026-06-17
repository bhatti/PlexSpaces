#!/bin/bash
# Test Insurance Claim Workflow - Zeebe/Camunda style (Python WASM)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/claim_workflow_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-zeebe-claim-py"
ACTOR_TYPE="claim"
NUM_CLAIMS=40

GREEN='\033[0;32m'
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

echo "================================================================"
echo "  Zeebe/Camunda → PlexSpaces: Insurance Claim Workflow (Python)"
echo "  Workflow + reminder facet; SLA escalation"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
  echo ""
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 1: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=migrating-zeebe-claim-py" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
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
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
  local id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$id/ask?timeout=$timeout" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Submit claim (workflow_run)"
RUN1=$(send_op "claim-1" '{"op":"workflow_run","claim_id":"claim-1"}' 15)
if echo "$RUN1" | grep -qE '"status":"(validated|pending_review)"'; then
  echo -e "  ${GREEN}Claim claim-1 submitted${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 120)..."
fi

echo "Step 3: Validate (second run)"
RUN2=$(send_op "claim-1" '{"op":"workflow_run","claim_id":"claim-1"}' 15)
if echo "$RUN2" | grep -q '"status":"pending_review"'; then
  echo -e "  ${GREEN}Claim in pending_review${NC}"
fi

echo "Step 4: Approve (action=approve)"
RUN3=$(send_op "claim-1" '{"op":"workflow_run","claim_id":"claim-1","action":"approve"}' 15)
if echo "$RUN3" | grep -q '"status":"approved"'; then
  echo -e "  ${GREEN}Claim approved${NC}"
fi

echo "Step 5: Escalation (signal escalate) – claim-2"
send_op "claim-2" '{"op":"workflow_run","claim_id":"claim-2"}' 15 >/dev/null
send_op "claim-2" '{"op":"workflow_run","claim_id":"claim-2"}' 15 >/dev/null
send_op "claim-2" '{"op":"workflow_signal:escalate"}' 5 >/dev/null
RUN_ESC=$(send_op "claim-2" '{"op":"workflow_run","claim_id":"claim-2"}' 15)
if echo "$RUN_ESC" | grep -q '"status":"escalated"'; then
  echo -e "  ${GREEN}Claim-2 escalated after signal${NC}"
fi

echo "Step 6: Query status"
send_op "claim-1" '{"op":"workflow_query:status"}' 5 | head -c 120
echo "..."

echo "Step 7: Batch $NUM_CLAIMS claims (submit → validate → approve)"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $NUM_CLAIMS); do
  send_op "batch-$i" "{\"op\":\"workflow_run\",\"claim_id\":\"batch-$i\"}" 10 >/dev/null
  send_op "batch-$i" "{\"op\":\"workflow_run\",\"claim_id\":\"batch-$i\"}" 10 >/dev/null
  RESP=$(send_op "batch-$i" "{\"op\":\"workflow_run\",\"claim_id\":\"batch-$i\",\"action\":\"approve\"}" 10)
  if [ "$i" -eq "$NUM_CLAIMS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Claims/sec: $(echo "scale=1; $NUM_CLAIMS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 8: Metrics from last run"
echo "================================================================"
STATS_RESP="$LAST_RUN"
if echo "$STATS_RESP" | grep -q '"claim_id"'; then
  export WALL_MS NUM_CLAIMS
  echo "$STATS_RESP" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
claim_id = p.get('claim_id', 'N/A')
status = p.get('status', 'N/A')
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
steps = p.get('steps_completed', 0) or 0
total_ms = compute_ms + coord_ms
if total_ms > 0:
  compute_pct = 100 * compute_ms / total_ms
  coord_pct = 100 * coord_ms / total_ms
  gran = compute_ms / coord_ms if coord_ms > 0 else 0
else:
  compute_pct = coord_pct = gran = 0
wall_ms = int(os.environ.get('WALL_MS', 0))
n = int(os.environ.get('NUM_CLAIMS', 1))
per_sec = n / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print('  Claim workflow (sample)')
print('  ────────────────────────────────────────────')
print(f'  Claim ID:          {claim_id}')
print(f'  Status:            {status}')
print(f'  Steps completed:   {steps}')
print()
print('  Benchmarks (coord vs compute)')
print('  ────────────────────────────────────────────')
print(f'  Compute time:      {compute_ms:.1f}ms ({compute_pct:.1f}%)')
print(f'  Coordination:      {coord_ms:.1f}ms ({coord_pct:.1f}%)')
print(f'  Granularity:       {gran:.1f}x')
print()
print(f'  Batch wall:        {wall_ms}ms')
print(f'  Claims:            {n}')
print(f'  Claims/sec:       {per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $STATS_RESP)"
else
  echo "  Response: $STATS_RESP"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Insurance Claim Workflow (Zeebe) Test Complete${NC}"
echo "================================================================"
