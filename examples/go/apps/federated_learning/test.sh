#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_GO_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
source "${_GO_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/federated_learning_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"; fi

if [[ -z "${1:-}" ]]; then NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then NODES=""; for _p in "$@"; do NODES="${NODES:+$NODES }localhost:$_p"; done
else NODES="$*"; NODES="${NODES//,/ }"; fi

GREEN='\033[0;32m'; RED='\033[0;31m'; BOLD='\033[1m'; NC='\033[0m'
APP_ID="go-federated-learning"; APP_NAME="go-federated-learning"
SCALING_CLIENTS="${SCALING_CLIENTS:-2,4,8,16}"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"; ENTRY_HOST="${ENTRY_NODE%%:*}"; ENTRY_PORT="${ENTRY_NODE##*:}"
LEADER_ACTOR="aggregator"

grpc_seed_nodes() { local j="" s=""; for e in "${NODE_LIST[@]}"; do j="${j}${s}\"${e}\""; s=", "; done; printf '%s' "$j"; }
render_config() {
  python3 - "$CONFIG_FILE" "$1" "$(grpc_seed_nodes)" <<'PY'
import pathlib, sys
src = pathlib.Path(sys.argv[1]).read_text(); seeds = sys.argv[3]; lines = []
for line in src.splitlines():
    lines.append(f"seed_nodes = [{seeds}]" if line.startswith("seed_nodes = ") else line)
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY
}
require_nonempty() { if [[ -z "$2" ]]; then echo -e "${RED}FAIL [$1]${NC}"; exit 1; fi; }
ask_actor() {
  curl -s --max-time "$2" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$1/ask?timeout=$2" \
    -H "Content-Type: application/json" ${AUTH_HEADER:+-H "$AUTH_HEADER"} -d "$3" 2>/dev/null || echo '{"error":"timeout"}'
}

if [ ! -f "$WASM_FILE" ] || find "$SCRIPT_DIR" -maxdepth 3 -name '*.go' -newer "$WASM_FILE" -print -quit 2>/dev/null | grep -q .; then
  "$SCRIPT_DIR/build.sh"
fi

for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"; p="${node##*:}"
  hc=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || hc="000"
  if [ "$hc" = "000" ]; then echo -e "${RED}Cannot connect to ${h}:${p}${NC}"; exit 1; fi
done

echo -e "${BOLD}Step 1: Deploy${NC}"
TEMP_DIR="$(mktemp -d)"; TEMP_CONFIG="$TEMP_DIR/app-config.toml"
trap 'rm -rf "${TEMP_DIR:-}"' EXIT
APP_ZIP="$TEMP_DIR/app.zip"
render_config "$TEMP_CONFIG"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
"$SCRIPT_DIR/undeploy.sh" $NODES; sleep 2

for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then continue; fi
  host="${node%%:*}"; port="${node##*:}"; _dep=0
  for _a in 1 2 3; do
    resp=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
      "http://${host}:${port}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" -F "app_file=@$APP_ZIP" 2>&1) || true
    hc=$(echo "$resp" | tail -n1); body=$(echo "$resp" | sed '$d')
    if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi; sleep 3
  done
  if [ "$_dep" -eq 0 ]; then echo -e "${RED}Deploy failed${NC}"; exit 1; fi
done
_dep=0
for _a in 1 2 3; do
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  resp=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" -F "app_file=@$APP_ZIP" 2>&1) || true
  hc=$(echo "$resp" | tail -n1); body=$(echo "$resp" | sed '$d')
  if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi; sleep 3
done
if [ "$_dep" -eq 0 ]; then echo -e "${RED}Deploy failed${NC}"; exit 1; fi
echo -e "${GREEN}Deployed${NC}"
if [ "${#NODE_LIST[@]}" -gt 1 ]; then wait_for_registry_membership "${#NODE_LIST[@]}" 10 3; fi

echo -e "\n${BOLD}Step 2: Basic training (4 clients, 20 rounds, 50000 samples/client)${NC}"
BASIC=""
_basic_start=$(date +%s%N)
for _a in $(seq 1 10); do
  BASIC=$(ask_actor "$LEADER_ACTOR" 300 '{"op":"run","client_count":4,"rounds":20,"samples_per_client":50000,"local_epochs":20,"dp_epsilon":1.0}')
  if echo "$BASIC" | grep -q '"status":"ok"'; then break; fi
  sleep 5
done
require_nonempty "basic" "$BASIC"
_basic_end=$(date +%s%N)
BASIC_WALL_MS=$(( (_basic_end - _basic_start) / 1000000 ))

if ! echo "$BASIC" | grep -q '"status":"ok"'; then
  echo -e "${RED}Basic run failed: $BASIC${NC}"; exit 1
fi

export BASIC_RESPONSE="$BASIC"
export BASIC_WALL_MS
RUN_RESPONSE="$BASIC" python3 - <<'PY'
import json, os
raw = json.loads(os.environ["RUN_RESPONSE"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert payload.get("status") == "ok", f"expected status=ok, got: {payload.get('status')} error={payload.get('error','')}"
w = int(os.environ.get("BASIC_WALL_MS", "0"))
c = int(payload.get("compute_time_ms", 0))
co = int(payload.get("coordination_time_ms", 0))
t = c + co or 1
print("━" * 72)
print("  Federated Learning (Go WASM)")
print(f"  client_count={payload.get('client_count',0)} rounds_completed={payload.get('rounds_completed',0)} samples_per_client={payload.get('samples_per_client',0)}")
print(f"  final_accuracy={payload.get('final_accuracy',0):.4f} final_loss={payload.get('final_loss',0):.4f} dp_epsilon={payload.get('dp_epsilon',0)}")
print(f"  wall_time_ms={w} compute_time_ms={c} ({c*100//t}%) coordination_time_ms={co} ({co*100//t}%)")
print(f"  granularity_ratio={payload.get('granularity_ratio',0):.2f}x actor_count={payload.get('actor_count',0)} errors={payload.get('error_count',0)}")
print("━" * 72)
assert payload.get("rounds_completed", 0) > 0
assert payload.get("error_count", 0) == 0
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 2 passed${NC}"

echo -e "\n${BOLD}Step 3: Strong scaling${NC}"
SP=$(python3 -c "import json; print(json.dumps({'op':'run_scaling_benchmark','client_counts':[int(s) for s in '${SCALING_CLIENTS}'.split(',')],'rounds':10,'samples_per_client':10000,'local_epochs':10,'warmup_rounds':1,'benchmark_rounds':2}))")
SR=$(ask_actor "$LEADER_ACTOR" 300 "$SP")
require_nonempty "scaling" "$SR"

if ! echo "$SR" | grep -q '"status":"ok"'; then
  echo -e "${RED}Scaling run failed: $SR${NC}"; exit 1
fi

RUN_RESPONSE="$SR" python3 - <<'PY'
import json, os
raw = json.loads(os.environ["RUN_RESPONSE"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)
assert payload.get("status") == "ok"
print("━" * 90)
print("  Federated Learning (Go) — Strong Scaling")
print("━" * 90)
print(f"  {'Clients':>8} {'Wall ms':>8} {'Comp ms':>8} {'Coord ms':>9} {'Gran':>6} {'Speedup':>8} {'Eff%':>6} {'Acc':>8} {'Errs':>5}")
print("─" * 90)
for row in payload.get("results", []):
    print(f"  {row.get('clients',0):>8} {row.get('wall_time_ms',0):>8} {row.get('compute_time_ms',0):>8} {row.get('coordination_time_ms',0):>9} {row.get('granularity_ratio',0):>6.1f} {row.get('speedup',0):>7.2f}x {row.get('efficiency_pct',0):>5.1f}% {row.get('final_accuracy',0):>7.4f} {row.get('error_count',0):>5}")
print("━" * 90)
for row in payload.get("results", []):
    assert row.get("error_count", 0) == 0
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 3 passed${NC}"

echo ""
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}  Federated Learning (Go) — All steps passed${NC}"
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
