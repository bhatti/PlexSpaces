#!/usr/bin/env bash
# Test parameter sweep — Merlin style (Rust WASM): pool + tuple space; fallback process group.
# Usage: ./test.sh [HTTP_PORT]
#
# Pass JSON to Python via env (METRICS_JSON=...), not `echo | python3 - <<'PY'`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/sweep_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="migrating-merlin-sweep-rust"
ACTOR_TYPE="sweep"
NUM_PARAMS=80
BATCH_SWEEPS=10
BATCH_PARAMS_PER_SWEEP=24
POOL_SIZE_FIRST=2

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  Merlin → PlexSpaces: Parameter sweep (worker pool, elastic)"
echo "================================================================"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Missing $WASM_FILE after build${NC}"
  exit 1
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
  -F "name=migrating-merlin-sweep-rust" \
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

assert_actor_ok() {
  local step_name="$1"
  local json_raw="$2"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except json.JSONDecodeError as e:
    print(f"FAIL [{step}]: invalid JSON: {e}", file=sys.stderr)
    sys.exit(1)

def deep_err(obj):
    if isinstance(obj, dict):
        if obj.get("status") == "error":
            p = obj.get("payload")
            if isinstance(p, str):
                try:
                    p = json.loads(p)
                except Exception:
                    return str(obj.get("error") or obj)
            if isinstance(p, dict) and p.get("error"):
                return str(p.get("error"))
            if obj.get("error"):
                return str(obj.get("error"))
            return str(obj.get("error_message") or "error")
        for v in obj.values():
            e = deep_err(v)
            if e:
                return e
    elif isinstance(obj, list):
        for x in obj:
            e = deep_err(x)
            if e:
                return e
    return None

err = deep_err(d)
if err:
    print(f"FAIL [{step}]: {err}", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
PY
  then
    echo -e "${RED}Assertion failed: $step_name${NC}"
    echo "  Raw: $(echo "$json_raw" | head -c 400)"
    exit 1
  fi
}

echo "Step 2: Wake worker pool (size $POOL_SIZE_FIRST)"
for i in $(seq 0 $((POOL_SIZE_FIRST - 1))); do
  W=$(send_op "worker-$i" '{"op":"work_available","sweep_id":"","num_params":0}' 10)
  assert_actor_ok "worker-$i warmup" "$W"
done
echo -e "  ${GREEN}Pool size: $POOL_SIZE_FIRST workers${NC}"

echo "Step 3: Run parameter sweep (coord-1 scatters $NUM_PARAMS, workers take/run/write; coord gathers)"
RUN1=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"sweep_id\":\"sweep-1\",\"num_params\":$NUM_PARAMS}" 45)
assert_actor_ok "workflow_run sweep-1" "$RUN1"
if ! echo "$RUN1" | grep -q '"status":"completed"'; then
  echo -e "${RED}Expected status completed${NC}"
  echo "$RUN1"
  exit 1
fi
echo -e "  ${GREEN}Sweep sweep-1 completed ($NUM_PARAMS params, pool=$POOL_SIZE_FIRST)${NC}"

echo "Step 4: Query coordinator status"
Q1=$(send_op "coord-1" '{"op":"workflow_query:status"}' 5)
assert_actor_ok "workflow_query:status" "$Q1"
echo "  Query: $(echo "$Q1" | head -c 160)..."

echo "Step 5: Second sweep"
RUN2=$(send_op "coord-1" '{"op":"workflow_run","sweep_id":"sweep-2","num_params":5}' 20)
assert_actor_ok "workflow_run sweep-2" "$RUN2"
if ! echo "$RUN2" | grep -q '"num_completed"'; then
  echo -e "${RED}Expected num_completed in response${NC}"
  echo "$RUN2"
  exit 1
fi
echo -e "  ${GREEN}Sweep sweep-2 run done${NC}"

echo "Step 6: Batch $BATCH_SWEEPS sweeps"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 "$BATCH_SWEEPS"); do
  RESP=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"sweep_id\":\"batch-$i\",\"num_params\":$BATCH_PARAMS_PER_SWEEP}" 30)
  assert_actor_ok "batch sweep $i" "$RESP"
  if [ "$i" -eq "$BATCH_SWEEPS" ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Sweeps: $BATCH_SWEEPS${NC}"

echo "Step 7: Metrics (last workflow + application list)"
if [ -z "$LAST_RUN" ]; then
  echo -e "${RED}No last run response${NC}"
  exit 1
fi
assert_actor_ok "last batch response" "$LAST_RUN"

APP_LIST_JSON=$(fetch_applications_list_json "http://localhost:$HTTP_PORT")

export WALL_MS METRICS_JSON APP_LIST_JSON APP_ID HTTP_PORT
if ! METRICS_JSON="$LAST_RUN" APP_LIST_JSON="$APP_LIST_JSON" WALL_MS="$WALL_MS" APP_ID="$APP_ID" HTTP_PORT="$HTTP_PORT" python3 - <<'PY'
import json
import os
import sys
from typing import Any, Dict, Optional


def unwrap_actor_payload(raw: str) -> Dict[str, Any]:
    raw = raw.strip()
    if not raw:
        raise ValueError("empty HTTP body")
    top = json.loads(raw)
    p = top.get("payload", top)
    if isinstance(p, str):
        p = p.strip()
        if not p:
            return {}
        try:
            p = json.loads(p)
        except json.JSONDecodeError:
            return {}
    return p if isinstance(p, dict) else {}


def find_app_record(data: Any, app_id: str) -> Optional[Dict[str, Any]]:
    if isinstance(data, dict):
        apps = data.get("applications", data.get("items"))
        if apps is None and "application_id" in data:
            apps = [data]
    elif isinstance(data, list):
        apps = data
    else:
        apps = []
    if not isinstance(apps, list):
        return None
    for a in apps:
        if not isinstance(a, dict):
            continue
        aid = str(a.get("application_id") or a.get("name") or a.get("id") or "")
        if aid == app_id or app_id in aid:
            return a
    return None


raw = os.environ.get("METRICS_JSON", "")
try:
    p = unwrap_actor_payload(raw)
except (json.JSONDecodeError, ValueError) as e:
    print(f"ERROR: parse last run: {e}", file=sys.stderr)
    sys.exit(1)

sid = p.get("sweep_id", "N/A")
status = p.get("status", "N/A")
num_params = p.get("num_params", 0)
num_done = p.get("num_completed", 0)
compute_ms = float(p.get("total_compute_ms", 0) or 0)
coord_ms = float(p.get("total_coord_ms", 0) or 0)
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
wall_ms = int(os.environ.get("WALL_MS", 0))
ds = p.get("data_size_bytes")

print()
print("  Merlin sweep (last workflow_run payload)")
print("  ────────────────────────────────────────────")
print(f"  Sweep ID:        {sid}")
print(f"  Status:          {status}")
print(f"  Params done:     {num_done} / {num_params}")
if ds is not None:
    print(f"  data_size_bytes: {ds}")
print()
print("  Benchmarks")
print("  ────────────────────────────────────────────")
print(f"  Compute ms:      {compute_ms:.1f} ({compute_pct:.1f}%)")
print(f"  Coord ms:        {coord_ms:.1f} ({coord_pct:.1f}%)")
print(f"  Batch wall:      {wall_ms}ms")

app_id = os.environ.get("APP_ID", "")
raw_list = os.environ.get("APP_LIST_JSON", "[]").strip() or "[]"
try:
    data = json.loads(raw_list)
except json.JSONDecodeError:
    print()
    print("  Application metrics (HTTP): could not parse applications list (non-fatal)")
    sys.exit(0)

found = find_app_record(data, app_id)
if not found:
    print()
    print(f"  Application metrics: no list entry for {app_id!r} (non-fatal)")
    sys.exit(0)

inner = found.get("application") if isinstance(found.get("application"), dict) else found
m = inner.get("metrics") if isinstance(inner, dict) else None
if not isinstance(m, dict):
    m = found.get("metrics") if isinstance(found.get("metrics"), dict) else {}

print()
print("  Application metrics (host::application_metrics_add)")
print("  ────────────────────────────────────────────")
if not m:
    print("  (no metrics on application record)")
    sys.exit(0)

mc = m.get("message_count")
ec = m.get("error_count")
if mc is not None or ec is not None:
    print(f"  message_count: {mc}  error_count: {ec}")

cm = m.get("counter_metrics")
if isinstance(cm, dict) and cm:
    print("  counter_metrics (sweep_*)")
    for k in sorted(cm.keys()):
        if k.startswith("sweep"):
            print(f"    {k}: {cm[k]}")

for key in ("latency_totals_ms", "latency_max_ms"):
    sub = m.get(key)
    if isinstance(sub, dict):
        rows = [(k, v) for k, v in sub.items() if "sweep" in str(k)]
        if rows:
            print(f"  {key} (sweep*)")
            for k, v in sorted(rows):
                print(f"    {k}: {v}")
PY
then
  echo -e "${RED}Step 7 metrics report failed${NC}"
  exit 1
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Merlin parameter sweep (Rust) test complete${NC}"
echo "================================================================"
