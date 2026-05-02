#!/bin/bash
# Test HPC Ensemble - EFlows4HPC style (Go WASM)
# Single actor type: ensemble:coord-1 (coordinator), ensemble:worker-0/1 (workers)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/ensemble_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-eflows4hpc-ensemble-go"
ACTOR_TYPE="ensemble"
NUM_TASKS=20

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  EFlows4HPC → PlexSpaces: HPC Ensemble (Go WASM)"
echo "  host.TS() (tuple space) + process group"
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
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=migrating-eflows4hpc-ensemble-go" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"; exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
  local instance_id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${instance_id}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Wake workers (join process group)"
send_op "worker-0" '{"op":"tasks_ready","ensemble_id":"","num_tasks":0}' 10 >/dev/null
send_op "worker-1" '{"op":"tasks_ready","ensemble_id":"","num_tasks":0}' 10 >/dev/null
echo -e "  ${GREEN}Workers woken${NC}"

echo "Step 3: Run ensemble (coordinator: coord-1 scatters $NUM_TASKS, broadcasts; workers take from tuple space; coordinator gathers)"
RUN1=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"ensemble_id\":\"ens-1\",\"num_tasks\":$NUM_TASKS}" 45)
if echo "$RUN1" | grep -q '"status":"completed"'; then
  echo -e "  ${GREEN}Ensemble ens-1 completed ($NUM_TASKS tasks)${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 200)..."
fi

echo "Step 4: Query coordinator status"
send_op "coord-1" '{"op":"workflow_query:status"}' 5 | head -c 140
echo "..."

echo "Step 5: Second ensemble"
RUN2=$(send_op "coord-1" '{"op":"workflow_run","ensemble_id":"ens-2","num_tasks":5}' 20)
if echo "$RUN2" | grep -q '"num_completed"'; then
  echo -e "  ${GREEN}Ensemble ens-2 run done${NC}"
fi

echo "Step 6: Batch 10 ensembles"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 10); do
  RESP=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"ensemble_id\":\"batch-$i\",\"num_tasks\":8}" 30)
  if [ "$i" -eq 10 ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Ensembles: 10${NC}"
echo ""

echo "Step 7: Metrics from last run"
echo "================================================================"
if [ -n "$LAST_RUN" ] && echo "$LAST_RUN" | grep -q '"ensemble_id"'; then
  export WALL_MS
  echo "$LAST_RUN" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
eid = p.get('ensemble_id', 'N/A')
status = p.get('status', 'N/A')
num_tasks = p.get('num_tasks', 0)
num_done = p.get('num_completed', 0)
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
wall_ms = int(os.environ.get('WALL_MS', 0))
print()
print('  HPC Ensemble (EFlows4HPC-style)')
print('  host.TS() + process group')
print('  ────────────────────────────────────────────')
print(f'  Ensemble ID:     {eid}')
print(f'  Status:          {status}')
print(f'  Tasks:           {num_done} / {num_tasks}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Compute ms:      {compute_ms:.1f} ({compute_pct:.1f}%)')
print(f'  Coord ms:        {coord_ms:.1f} ({coord_pct:.1f}%)')
print(f'  Batch wall:      {wall_ms}ms')
" 2>/dev/null || echo "  (Parse: $LAST_RUN)"
else
  echo "  Response: $LAST_RUN"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}HPC Ensemble (EFlows4HPC) Test Complete${NC}"
echo "================================================================"
