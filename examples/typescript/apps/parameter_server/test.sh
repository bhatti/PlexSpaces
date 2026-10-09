#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Integration test for the TypeScript parameter server distributed ML example.
#
# Usage:
#   bash test.sh [port ...]         # default: 8091 8094
#
# Environment overrides:
#   NODES, ITERATIONS, WORKER_COUNT, BATCH_SIZE, INPUT_DIM, HIDDEN_DIM,
#   KEEP_DEPLOYED (default 1 — keep deployed after test)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/parameter_server_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES=""
  for _port in "$@"; do NODES="${NODES:+$NODES }localhost:$_port"; done
else
  NODES="$*"; NODES="${NODES//,/ }"
fi

APP_ID="ts-parameter-server"
APP_NAME="ts-parameter-server"
LEADER_ACTOR="leader"
ITERATIONS="${ITERATIONS:-20}"
WORKER_COUNT="${WORKER_COUNT:-8}"
BATCH_SIZE="${BATCH_SIZE:-512}"
INPUT_DIM="${INPUT_DIM:-100}"
HIDDEN_DIM="${HIDDEN_DIM:-64}"
SCALING_WORKERS="${SCALING_WORKERS:-2,4,8,16}"

GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'; NC='\033[0m'

if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi
echo "AUTH_HEADER set: $([ -n "$AUTH_HEADER" ] && echo YES || echo NO)"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"

grpc_seed_nodes() {
  local result=""
  for _n in "${NODE_LIST[@]}"; do
    local _h="${_n%%:*}"; local _p="${_n##*:}"
    [ -n "$result" ] && result="${result}, "
    result="${result}\"${_h}:${_p}\""
  done
  printf '%s' "$result"
}

render_config() {
  local temp_config="$1"
  local seeds; seeds="$(grpc_seed_nodes)"
  python3 - "$CONFIG_FILE" "$temp_config" "$seeds" <<'PY'
import pathlib, sys
src = pathlib.Path(sys.argv[1]).read_text(); seeds = sys.argv[3]; lines = []
for line in src.splitlines():
    lines.append(f"seed_nodes = [{seeds}]" if line.startswith("seed_nodes = ") else line)
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY
}

echo "Step 0: Build WASM"
if [ ! -f "$WASM_FILE" ] || find "$SCRIPT_DIR" -maxdepth 1 \( -name '*.ts' \) -newer "$WASM_FILE" -print -quit 2>/dev/null | grep -q .; then
  bash "$SCRIPT_DIR/build.sh"
else
  echo "  (WASM up-to-date)"
fi
[ -f "$WASM_FILE" ] || { echo -e "${RED}Build did not produce $WASM_FILE${NC}"; exit 1; }
echo ""

for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"; p="${node##*:}"
  hc=$(curl -s --max-time 5 -o /dev/null -w "%{http_code}" ${AUTH_HEADER:+-H "$AUTH_HEADER"} "http://${h}:${p}/" 2>/dev/null) || hc="000"
  if [ "$hc" = "000" ]; then echo -e "${RED}Cannot reach node at ${h}:${p}${NC}"; exit 1; fi
done

TEMP_DIR="$(mktemp -d)"
TEMP_CONFIG="$TEMP_DIR/app-config.toml"
APP_ZIP="$(mktemp).zip"
trap 'rm -rf "${TEMP_DIR:-}" "${APP_ZIP:-}"' EXIT
render_config "$TEMP_CONFIG"
zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null

echo "Step 1: Undeploy from all nodes, then deploy to all nodes"
bash "$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2

for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then continue; fi
  h="${node%%:*}"; p="${node##*:}"; _dep=0
  for _a in 1 2 3; do
    APP_ZIP="$(mktemp).zip"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
    out=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
      "http://${h}:${p}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" \
      -F "app_file=@$APP_ZIP" 2>&1) || true
    hc=$(echo "$out" | tail -n1); body=$(echo "$out" | sed '$d')
    if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi
    sleep 3
  done
  [ "$_dep" -eq 1 ] || { echo -e "${RED}Deploy to ${h}:${p} failed${NC}"; exit 1; }
done

_dep=0
for _a in 1 2 3; do
  APP_ZIP="$(mktemp).zip"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  out=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  hc=$(echo "$out" | tail -n1); body=$(echo "$out" | sed '$d')
  if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi
  sleep 3
done
[ "$_dep" -eq 1 ] || { echo -e "${RED}Deploy to ${ENTRY_NODE} failed${NC}"; exit 1; }
echo -e "  ${GREEN}Deployed${NC}"

if [ "${#NODE_LIST[@]}" -gt 1 ]; then
  echo "Step 1b: Waiting for cluster node discovery..."
  for _i in $(seq 1 10); do
    sleep 2
    MEMBERS=$(curl -s --max-time 5 ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/${APP_ID}/status" 2>/dev/null | \
      python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d.get('node_members',d.get('nodes',[]))))" 2>/dev/null || echo "0")
    if [ "$MEMBERS" -ge "${#NODE_LIST[@]}" ] 2>/dev/null; then
      echo -e "  ${GREEN}All ${#NODE_LIST[@]} nodes discovered${NC}"
      break
    fi
    echo -e "  ${YELLOW}Waiting for nodes... ($MEMBERS/${#NODE_LIST[@]})${NC}"
  done
fi

echo "Step 2: Trigger training on ${ENTRY_NODE}"
run_payload="{\"op\":\"train\",\"iterations\":${ITERATIONS}}"
run_start=$(python3 -c "import time; print(int(time.time()*1000))")
run_response=""
for _a in $(seq 1 10); do
  run_response=$(curl -s --max-time 240 -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/${APP_ID}/${LEADER_ACTOR}/ask?timeout=240" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} -H "Content-Type: application/json" \
    -d "$run_payload" 2>/dev/null || echo '{"error":"timeout"}')
  if echo "$run_response" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"'; then break; fi
  if echo "$run_response" | grep -q "Placement produced no target nodes"; then
    echo -e "  ${YELLOW}Seed nodes not reconciled yet, retrying...${NC}"; sleep 3; continue
  fi
  break
done
run_end=$(python3 -c "import time; print(int(time.time()*1000))")
wall_ms=$(( run_end - run_start ))

if ! echo "$run_response" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"'; then
  echo -e "${RED}Training failed: $run_response${NC}"
  exit 1
fi

echo "Step 3: Metrics"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
PARAMETER_SERVER_WALL_MS="$wall_ms" \
PARAMETER_SERVER_EXPECTED_NODES="${#NODE_LIST[@]}" \
RUN_RESPONSE="$run_response" python3 - <<'PY' || { echo -e "${RED}Metrics validation failed${NC}"; exit 1; }
import json, os, sys

raw = json.loads(os.environ["RUN_RESPONSE"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)

compute_ms   = int(payload.get("compute_time_ms", 0))
coord_ms     = int(payload.get("coordination_time_ms", 0))
wall_ms      = int(os.environ.get("PARAMETER_SERVER_WALL_MS", "0"))
total        = compute_ms + coord_ms
compute_pct  = (compute_ms * 100.0 / total) if total else 0.0
coord_pct    = (coord_ms * 100.0 / total) if total else 0.0
node_count   = int(payload.get("node_count", 0))
actor_count  = int(payload.get("actor_count", 0))
gradient_ops = int(payload.get("gradient_operation_count", 0))
samples      = int(payload.get("samples_processed", 0))
weight_upd   = int(payload.get("weight_update_count", 0))
errors       = int(payload.get("error_count", 0))
expected_n   = int(os.environ.get("PARAMETER_SERVER_EXPECTED_NODES", "1"))

print("  Parameter Server (TypeScript WASM)")
print(f"  param_count={payload.get('param_count',0)}  worker_count={payload.get('worker_count',0)}  iterations={payload.get('iterations',0)}  batch_size={payload.get('batch_size',0)}")
print(f"  node_count={node_count}  actor_count={actor_count}  leader_node={payload.get('leader_node_id','')}")
print(f"  wall_ms={wall_ms}  compute_ms={compute_ms} ({compute_pct:.1f}%)  coord_ms={coord_ms} ({coord_pct:.1f}%)")
print(f"  granularity={float(payload.get('granularity_ratio',0)):.2f}x")
print(f"  avg_worker_latency_ms={float(payload.get('avg_worker_latency_ms',0)):.2f}  max_worker_latency_ms={float(payload.get('max_worker_latency_ms',0)):.2f}")
print(f"  gradient_ops={gradient_ops}  samples_processed={samples}  weight_updates={weight_upd}")
print(f"  weight_checksum={payload.get('weight_checksum',0)}  errors={errors}")
print(f"  remote_nodes_with_work={payload.get('remote_nodes_with_work',[])}")

assert errors == 0,     f"expected zero errors but saw {errors}"
assert gradient_ops > 0, f"expected positive gradient_operation_count but got {gradient_ops}"
assert samples > 0,      f"expected positive samples_processed but got {samples}"
assert weight_upd > 0,   f"expected positive weight_update_count but got {weight_upd}"
PY
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

echo ""
echo "Step 4: Strong scaling benchmark (workers: ${SCALING_WORKERS})"
scale_workers_json=$(python3 -c "print('[' + ','.join(str(w) for w in '${SCALING_WORKERS}'.split(',')) + ']')")
scale_payload="{\"op\":\"run_scaling_benchmark\",\"worker_counts\":${scale_workers_json},\"iterations\":5,\"warmup_rounds\":1,\"benchmark_rounds\":2}"
scale_response=$(curl -s --max-time 300 -X POST \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/${APP_ID}/${LEADER_ACTOR}/ask?timeout=300" \
  ${AUTH_HEADER:+-H "$AUTH_HEADER"} -H "Content-Type: application/json" \
  -d "$scale_payload" 2>/dev/null || echo '{"error":"timeout"}')

if echo "$scale_response" | grep -qE '"status"[[:space:]]*:[[:space:]]*"ok"'; then
  python3 - "$scale_response" <<'PY'
import json, sys

raw = json.loads(sys.argv[1])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)

results = payload.get("results", [])
print("═" * 96)
print(f"  Parameter Server (TypeScript) — Strong Scaling  param_count={payload.get('param_count',0)}  batch_size={payload.get('batch_size',0)}")
print("═" * 96)
print(f"  {'Workers':>8} {'Total ms':>9} {'Compute ms':>11} {'Coord ms':>9} {'Comp%':>6} {'Gran':>6} {'Smpl/s':>8} {'Speedup':>8} {'Eff%':>6} {'ParFrac':>8} {'Errs':>5}")
print("─" * 96)
for r in results:
    print(f"  {r.get('workers',0):>8} {r.get('total_time_ms',0):>9} {r.get('compute_time_ms',0):>11} "
          f"{r.get('coordination_time_ms',0):>9} {r.get('compute_pct',0):>5}% {r.get('granularity_ratio',0):>6.2f} "
          f"{r.get('samples_per_sec',0):>8} {r.get('speedup',1.0):>7.2f}x {r.get('efficiency_pct',0):>5}% "
          f"{r.get('parallel_fraction',0):>7.2f}  {r.get('error_count',0):>5}")
print("═" * 96)
for r in results:
    assert r.get("error_count", 0) == 0, f"errors in {r.get('workers')} worker run: {r.get('error_count')}"
print("  ✓ Scaling benchmark passed")
PY
  echo -e "${GREEN}Step 4 passed${NC}"
else
  echo -e "${YELLOW}Step 4 (scaling benchmark) did not return status:ok — skipping (non-fatal)${NC}"
  echo "  Response: $(echo "$scale_response" | head -c 200)"
fi

if [ "${KEEP_DEPLOYED:-1}" != "1" ]; then
  echo "Undeploying ${APP_ID}..."
  bash "$SCRIPT_DIR/undeploy.sh" $NODES
fi

echo -e "${GREEN}Parameter server (TypeScript) test passed.${NC}"
