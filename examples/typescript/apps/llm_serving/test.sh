#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_TS_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_TS_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/llm_serving_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

# Auto-generate JWT if not provided
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
APP_ID="ts-llm-serving"; APP_NAME="ts-llm-serving"
SCALING_SHARDS="${SCALING_SHARDS:-2,4,6,8}"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"; ENTRY_HOST="${ENTRY_NODE%%:*}"; ENTRY_PORT="${ENTRY_NODE##*:}"
LEADER_ACTOR_PATH="default:llm-leader"

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

if [ ! -f "$WASM_FILE" ] || find "$SCRIPT_DIR" -maxdepth 3 \( -name '*.ts' -not -name '*.test.*' \) -newer "$WASM_FILE" -print -quit 2>/dev/null | grep -q .; then
  "$SCRIPT_DIR/build.sh"
fi

for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"; p="${node##*:}"
  hc=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || hc="000"
  if [ "$hc" = "000" ]; then echo -e "${RED}Cannot connect to ${h}:${p}${NC}"; exit 1; fi
done

echo -e "${BOLD}Step 1: Deploy${NC}"
TEMP_DIR="$(mktemp -d)"; TEMP_CONFIG="$TEMP_DIR/app-config.toml"
trap 'rm -rf "${TEMP_DIR:-}" "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp).zip"
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
  APP_ZIP="$(mktemp).zip"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  resp=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" -F "app_file=@$APP_ZIP" 2>&1) || true
  hc=$(echo "$resp" | tail -n1); body=$(echo "$resp" | sed '$d')
  if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi; sleep 3
done
if [ "$_dep" -eq 0 ]; then echo -e "${RED}Deploy failed${NC}"; exit 1; fi
echo -e "${GREEN}Deployed${NC}"
if [ "${#NODE_LIST[@]}" -gt 1 ]; then wait_for_registry_membership "${#NODE_LIST[@]}" 10 3; fi

echo -e "\n${BOLD}Step 2: Basic LLM serving (1000 requests, 6 workers)${NC}"
BASIC=""
for _a in $(seq 1 10); do
  BASIC=$(ask_actor "$LEADER_ACTOR_PATH" 120 '{"op":"run","request_count":1000,"worker_count":6,"batch_size":16}')
  if echo "$BASIC" | grep -q '"status"[[:space:]]*:[[:space:]]*"ok"'; then break; fi; sleep 5
done
require_nonempty "basic" "$BASIC"

python3 - "$BASIC" <<'PY'
import json, sys
r = json.loads(sys.argv[1])
r = r.get("payload", r)
if isinstance(r, str): r = json.loads(r)
assert r.get("status") == "ok"
print("═" * 80); print("  LLM Serving (TypeScript) — Basic Run"); print("═" * 80)
w,c,co = r.get("wall_time_ms",0),r.get("compute_time_ms",0),r.get("coordination_time_ms",0)
t = c+co or 1
print(f"  Requests={r.get('request_count',0)}, workers={r.get('worker_count',0)}, batch={r.get('batch_size',0)}")
print(f"  wall={w}ms, compute={c}ms ({c*100//t}%), coord={co}ms ({co*100//t}%)")
print(f"  Granularity={r.get('granularity_ratio',0)}, req/s={r.get('requests_per_sec',0)}, tok/s={r.get('tokens_per_sec',0)}")
print(f"  Tokens={r.get('total_tokens',0)}, cost={r.get('total_cost',0)}, errors={r.get('failure_count',0)}")
print(f"  Avg latency={r.get('avg_latency_ms',0)}ms, max={r.get('max_latency_ms',0)}ms")
tr = r.get("tier_routing",{})
print(f"  Tier routing: small={tr.get('small',0)}, medium={tr.get('medium',0)}, large={tr.get('large',0)}")
print("═" * 80)
assert r.get("success_count",0) > 0; assert r.get("requests_per_sec",0) > 0
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 2 passed${NC}"

echo -e "\n${BOLD}Step 3: Strong scaling${NC}"
SP=$(python3 -c "import json; print(json.dumps({'op':'run_scaling_benchmark','request_count':1000,'shard_counts':[int(s) for s in '${SCALING_SHARDS}'.split(',')],'batch_size':16,'warmup_rounds':1,'benchmark_rounds':2}))")
SR=$(ask_actor "$LEADER_ACTOR_PATH" 300 "$SP"); require_nonempty "scaling" "$SR"

python3 - "$SR" <<'PY'
import json, sys
r = json.loads(sys.argv[1])
r = r.get("payload", r)
if isinstance(r, str): r = json.loads(r)
assert r.get("status") == "ok"
print("═" * 90); print("  LLM Serving (TypeScript) — Strong Scaling"); print("═" * 90)
print(f"  {'Shards':>8} {'Req/s':>8} {'Tok/s':>8} {'Wall':>8} {'Comp':>8} {'Coord':>9} {'Gran':>6} {'Speed':>8} {'Eff%':>6} {'Errs':>5}")
print("─" * 90)
for row in r.get("results",[]):
    print(f"  {row.get('shards',0):>8} {row.get('requests_per_sec',0):>8} {row.get('tokens_per_sec',0):>8} {row.get('wall_time_ms',0):>8} {row.get('compute_time_ms',0):>8} {row.get('coordination_time_ms',0):>9} {row.get('granularity_ratio',0):>6.1f} {row.get('speedup',0):>7.2f}x {row.get('efficiency_pct',0):>5.1f}% {row.get('error_count',0):>5}")
print("═" * 90)
for row in r.get("results",[]): assert row.get("error_count",0) < row.get("requests_per_sec",1)
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 3 passed${NC}"

echo -e "\n${BOLD}Step 4: Weak scaling${NC}"
WP=$(python3 -c "import json; print(json.dumps({'op':'run_weak_scaling_benchmark','requests_per_shard':200,'shard_counts':[int(s) for s in '${SCALING_SHARDS}'.split(',')],'num_passes':4,'benchmark_rounds':1}))")
WR=$(ask_actor "$LEADER_ACTOR_PATH" 300 "$WP"); require_nonempty "weak_scaling" "$WR"

python3 - "$WR" <<'PY'
import json, sys
r = json.loads(sys.argv[1])
r = r.get("payload", r)
if isinstance(r, str): r = json.loads(r)
assert r.get("status") == "ok"
print("═" * 90); print("  LLM Serving (TypeScript) — Weak Scaling (200 req/shard × 4 passes)"); print("═" * 90)
print(f"  {'Shards':>8} {'TotReq':>8} {'Req/s':>8} {'Tok/s':>8} {'Wall':>8} {'Comp':>8} {'Coord':>9} {'Gran':>6} {'Eff%':>6} {'Errs':>5}")
print("─" * 90)
for row in r.get("results",[]):
    print(f"  {row.get('shards',0):>8} {row.get('total_requests',0):>8} {row.get('requests_per_sec',0):>8} {row.get('tokens_per_sec',0):>8} {row.get('wall_time_ms',0):>8} {row.get('compute_time_ms',0):>8} {row.get('coordination_time_ms',0):>9} {row.get('granularity_ratio',0):>6.1f} {row.get('efficiency_pct',0):>5.1f}% {row.get('error_count',0):>5}")
print("═" * 90)
for row in r.get("results",[]): assert row.get("error_count",0) == 0
print("  ✓ Passed")
PY
echo -e "${GREEN}Step 4 passed${NC}"

echo ""
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}  LLM Serving (TypeScript) — All steps passed${NC}"
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
