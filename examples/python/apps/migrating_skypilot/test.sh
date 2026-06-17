#!/bin/bash
# Test Spot Job Runner - SkyPilot style (Python WASM)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/job_runner_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-skypilot-job-runner-py"
ACTOR_TYPE="job-runner"
NUM_JOBS=25

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
echo "  SkyPilot → PlexSpaces: Spot Job Runner (Python WASM)"
echo "  Checkpoint to KV; failover on preemption"
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
    -F "name=migrating-skypilot-job-runner-py" \
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

echo "Step 2: Run job to completion (no preemption)"
RUN1=$(send_op "job-1" '{"op":"workflow_run","job_id":"job-1","total_steps":4}' 15)
if echo "$RUN1" | grep -q '"status":"completed"'; then
  echo -e "  ${GREEN}Job job-1 completed (4 steps)${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 120)..."
fi

echo "Step 3: Simulate preemption then failover (job-2)"
send_op "job-2" '{"op":"workflow_run","job_id":"job-2","total_steps":5,"simulate_preemption_at_step":3}' 15 >/dev/null
RUN_RESUME=$(send_op "job-2" '{"op":"workflow_run","job_id":"job-2","total_steps":5}' 15)
if echo "$RUN_RESUME" | grep -q '"status":"completed"'; then
  echo -e "  ${GREEN}Job job-2 preempted at step 3, resumed from checkpoint and completed${NC}"
elif echo "$RUN_RESUME" | grep -q '"preempted_at_step"'; then
  echo -e "  ${GREEN}Job job-2 preempted (will complete on next run)${NC}"
else
  echo "  Response: $(echo "$RUN_RESUME" | head -c 100)..."
fi

echo "Step 4: Query status"
send_op "job-1" '{"op":"workflow_query:status"}' 5 | head -c 120
echo "..."

echo "Step 5: Batch $NUM_JOBS jobs"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $NUM_JOBS); do
  RESP=$(send_op "batch-$i" "{\"op\":\"workflow_run\",\"job_id\":\"batch-$i\",\"total_steps\":3}" 15)
  if [ "$i" -eq "$NUM_JOBS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Jobs: $NUM_JOBS${NC}"
echo ""

echo "Step 6: Metrics from last run"
echo "================================================================"
if [ -n "$LAST_RUN" ] && echo "$LAST_RUN" | grep -q '"job_id"'; then
  export WALL_MS NUM_JOBS
  echo "$LAST_RUN" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
job_id = p.get('job_id', 'N/A')
status = p.get('status', 'N/A')
current = p.get('current_step', 0)
total = p.get('total_steps', 0)
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
if total_ms > 0:
  compute_pct = 100 * compute_ms / total_ms
  coord_pct = 100 * coord_ms / total_ms
else:
  compute_pct = coord_pct = 0
wall_ms = int(os.environ.get('WALL_MS', 0))
n = int(os.environ.get('NUM_JOBS', 1))
per_sec = n / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print('  Spot Job Runner (SkyPilot-style)')
print('  ────────────────────────────────────────────')
print(f'  Job ID:           {job_id}')
print(f'  Status:           {status}')
print(f'  Steps:            {current} / {total}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Compute ms:       {compute_ms:.1f} ({compute_pct:.1f}%)')
print(f'  Coord ms:         {coord_ms:.1f} ({coord_pct:.1f}%)')
print(f'  Batch wall:       {wall_ms}ms')
print(f'  Jobs/sec:         {per_sec:.1f}')
" 2>/dev/null || echo "  (Parse: $LAST_RUN)"
else
  echo "  Response: $LAST_RUN"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Spot Job Runner (SkyPilot) Test Complete${NC}"
echo "================================================================"
