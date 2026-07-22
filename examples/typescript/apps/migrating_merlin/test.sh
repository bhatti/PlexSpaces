#!/bin/bash
# Test parameter sweep - Merlin style (TypeScript WASM), worker pool with elastic size
# Uses elastic pool API (poolCheckout/send/poolCheckin) when pool is configured;
# falls back to process group broadcast. sweep:coord-1 = coordinator; sweep:worker-0,1,... = workers.
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/sweep_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-merlin-sweep-ts"
ACTOR_TYPE="sweep"
NUM_PARAMS=80
BATCH_SWEEPS=10
BATCH_PARAMS_PER_SWEEP=24
POOL_SIZE_FIRST=2

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
echo "  Merlin → PlexSpaces: Parameter sweep (worker pool, elastic)"
echo "  Pool API (checkout/checkin) + host.ts (work queue); fallback: process group"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
  echo ""
fi

HTTP_CHECK="000"
for _i in 1 2 3; do
  trap 'rm -f "${APP_ZIP:-}"' EXIT
  APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
  HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
  [ "$HTTP_CHECK" != "000" ] && break
  sleep 2
done
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
    -F "name=migrating-merlin-sweep-ts" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
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
  local instance_id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${instance_id}/ask?timeout=$timeout" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Wake worker pool (size $POOL_SIZE_FIRST) – workers join merlin-workers"
for i in $(seq 0 $((POOL_SIZE_FIRST - 1))); do
  send_op "worker-$i" '{"op":"work_available","sweep_id":"","num_params":0}' 10 >/dev/null
done
echo -e "  ${GREEN}Pool size: $POOL_SIZE_FIRST workers${NC}"

echo "Step 3: Run parameter sweep (coord-1 scatters $NUM_PARAMS, workers take/run/write; coord gathers)"
RUN1=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"sweep_id\":\"sweep-1\",\"num_params\":$NUM_PARAMS}" 45)
if echo "$RUN1" | grep -q '"status":"completed"'; then
  echo -e "  ${GREEN}Sweep sweep-1 completed ($NUM_PARAMS params, pool=$POOL_SIZE_FIRST)${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 200)..."
fi

echo "Step 4: Query coordinator status"
send_op "coord-1" '{"op":"workflow_query:status"}' 5 | head -c 140
echo "..."

echo "Step 5: Second sweep"
RUN2=$(send_op "coord-1" '{"op":"workflow_run","sweep_id":"sweep-2","num_params":5}' 20)
if echo "$RUN2" | grep -q '"num_completed"'; then
  echo -e "  ${GREEN}Sweep sweep-2 run done${NC}"
fi

echo "Step 6: Batch $BATCH_SWEEPS sweeps"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 $BATCH_SWEEPS); do
  RESP=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"sweep_id\":\"batch-$i\",\"num_params\":$BATCH_PARAMS_PER_SWEEP}" 30)
  if [ "$i" -eq "$BATCH_SWEEPS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Sweeps: $BATCH_SWEEPS${NC}"
echo ""

echo "================================================================"
echo -e "  ${GREEN}Merlin parameter sweep (TypeScript) test complete${NC}"
echo "================================================================"
