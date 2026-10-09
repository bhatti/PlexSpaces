#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_PYTHON_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_PYTHON_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/cdc_pipeline_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES=""
  for _port in "$@"; do NODES="${NODES:+$NODES }localhost:$_port"; done
else
  NODES="$*"; NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'; RED='\033[0;31m'; BOLD='\033[1m'; NC='\033[0m'
APP_ID="python-cdc-pipeline"
APP_NAME="python-cdc-pipeline"
SCALING_SHARDS="${SCALING_SHARDS:-2,4,8,16}"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"
LEADER_ACTOR_PATH="default:LeaderActor"

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
require_nonempty() { if [[ -z "$2" ]]; then echo -e "${RED}FAIL [$1]: empty response${NC}"; exit 1; fi; }
ask_actor() {
  curl -s --max-time "$2" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$1/ask?timeout=$2" \
    -H "Content-Type: application/json" ${AUTH_HEADER:+-H "$AUTH_HEADER"} -d "$3" 2>/dev/null || echo '{"error":"timeout"}'
}

# BUILD
if [ ! -f "$WASM_FILE" ] || find "$SCRIPT_DIR" -maxdepth 3 -name '*.py' -newer "$WASM_FILE" -print -quit 2>/dev/null | grep -q .; then
  "$SCRIPT_DIR/build.sh"
fi

# CONNECTIVITY
for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"; p="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then echo -e "${RED}Cannot connect to ${h}:${p}${NC}"; exit 1; fi
done

# DEPLOY
echo -e "${BOLD}Step 1: Deploy${NC}"
TEMP_DIR="$(mktemp -d)"; TEMP_CONFIG="$TEMP_DIR/app-config.toml"
trap 'rm -rf "${TEMP_DIR:-}" "${APP_ZIP:-}"' EXIT
APP_ZIP="$TEMP_DIR/app.zip"
render_config "$TEMP_CONFIG"
zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
"$SCRIPT_DIR/undeploy.sh" $NODES; sleep 5

for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then continue; fi
  host="${node%%:*}"; port="${node##*:}"; _dep=0
  for _a in 1 2 3 4 5; do
    resp=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
      "http://${host}:${port}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" -F "app_file=@$APP_ZIP" 2>&1) || true
    hc=$(echo "$resp" | tail -n1); body=$(echo "$resp" | sed '$d')
    if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi
    sleep 5
  done
  if [ "$_dep" -eq 0 ]; then echo -e "${RED}Deploy to ${host}:${port} failed${NC}"; exit 1; fi
done
_dep=0
for _a in 1 2 3 4 5; do
  APP_ZIP="$TEMP_DIR/app.zip"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  resp=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" -F "app_file=@$APP_ZIP" 2>&1) || true
  hc=$(echo "$resp" | tail -n1); body=$(echo "$resp" | sed '$d')
  if [ "$hc" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi
  sleep 5
done
if [ "$_dep" -eq 0 ]; then echo -e "${RED}Deploy failed${NC}"; exit 1; fi
echo -e "${GREEN}Deployed${NC}"
if [ "${#NODE_LIST[@]}" -gt 1 ]; then wait_for_registry_membership "${#NODE_LIST[@]}" 10 3; fi

# Step 2: Basic Run
echo -e "\n${BOLD}Step 2: Basic CDC pipeline (10000 events, 8 workers)${NC}"
BASIC=""
for _a in $(seq 1 10); do
  BASIC=$(ask_actor "$LEADER_ACTOR_PATH" 120 '{"op":"run","event_count":10000,"worker_count":8,"batch_size":500}')
  if echo "$BASIC" | grep -q '"status"[[:space:]]*:[[:space:]]*"ok"'; then break; fi
  echo "  Attempt $_a: retrying..."; sleep 5
done
require_nonempty "basic_run" "$BASIC"

python3 - "$BASIC" <<'PY'
import json, sys
raw = json.loads(sys.argv[1])
r = raw.get("payload", raw)
if isinstance(r, str):
    r = json.loads(r)
assert r.get("status") == "ok", f"status={r.get('status')}"
print("═" * 72)
print("  CDC Pipeline — Basic Run Results")
print("═" * 72)
w = r.get("wall_time_ms", 0); c = r.get("compute_time_ms", 0); co = r.get("coordination_time_ms", 0)
t = c + co or 1
print(f"  Events:        {r.get('event_count', 0)}, workers={r.get('worker_count', 0)}")
print(f"  Timing:        wall={w}ms, compute={c}ms ({c*100//t}%), coord={co}ms ({co*100//t}%)")
print(f"  Granularity:   {r.get('granularity_ratio', 0)} (target ≥10x)")
print(f"  Throughput:    {r.get('events_per_sec', 0)} events/sec")
print(f"  Fan-out:       search={r.get('search_docs',0)}, analytics={r.get('analytics_records',0)}, cache={r.get('cache_invalidations',0)}")
print(f"  Dedup:         {r.get('duplicates_skipped', 0)} duplicates skipped")
print(f"  Tables:        {json.dumps(r.get('table_counts', {}))}")
print(f"  Errors:        {r.get('error_count', 0)}")
print("═" * 72)
assert r.get("search_docs", 0) > 0
assert r.get("analytics_records", 0) > 0
assert r.get("error_count", 0) == 0
print("  ✓ Basic run passed")
PY
echo -e "${GREEN}Step 2 passed${NC}"

# Step 3: Strong Scaling
echo -e "\n${BOLD}Step 3: Strong scaling benchmark${NC}"
SCALING_PAYLOAD=$(python3 -c "
import json; shards = [int(s) for s in '${SCALING_SHARDS}'.split(',')]
print(json.dumps({'op':'run_scaling_benchmark','event_count':10000,'shard_counts':shards,'batch_size':500,'warmup_rounds':1,'benchmark_rounds':2}))
")
SCALING=$(ask_actor "$LEADER_ACTOR_PATH" 300 "$SCALING_PAYLOAD")
require_nonempty "strong_scaling" "$SCALING"

python3 - "$SCALING" <<'PY'
import json, sys
raw = json.loads(sys.argv[1])
r = raw.get("payload", raw)
if isinstance(r, str):
    r = json.loads(r)
if r.get("status") != "ok":
    print(f"ERROR: {str(r)[:1000]}", file=sys.stderr)
    sys.exit(1)
results = r.get("results", [])
print("═" * 90)
print("  CDC Pipeline — Strong Scaling")
print("═" * 90)
print(f"  {'Workers':>8} {'Evt/s':>8} {'Wall ms':>8} {'Comp ms':>8} {'Coord ms':>9} {'Gran':>6} {'Speedup':>8} {'Eff%':>6} {'Errs':>5}")
print("─" * 90)
for row in results:
    print(f"  {row.get('shards',0):>8} {row.get('events_per_sec',0):>8} {row.get('wall_time_ms',0):>8} {row.get('compute_time_ms',0):>8} {row.get('coordination_time_ms',0):>9} {row.get('granularity_ratio',0):>6.1f} {row.get('speedup',0):>7.2f}x {row.get('efficiency_pct',0):>5.1f}% {row.get('error_count',0):>5}")
print("═" * 90)
for row in results: assert row.get("error_count", 0) == 0
print("  ✓ Strong scaling passed")
PY
echo -e "${GREEN}Step 3 passed${NC}"

echo ""
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}  CDC Pipeline (Python) — All steps passed${NC}"
echo -e "${GREEN}${BOLD}═══════════════════════════════════════════════════════════════════════${NC}"
