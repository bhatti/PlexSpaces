#!/usr/bin/env bash
# Test Order Fulfillment Workflow - Rust WASM
# Usage: ./test.sh [HTTP_PORT]
#
# Shell: pass JSON to Python via env (e.g. STATS_RESP=...), not `echo | python3 - <<'PY'`.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/order_fulfillment_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="temporal-order-fulfillment-rust"
ACTOR_TYPE="order-fulfillment"
NUM_ORDERS=50
NODE_ID="${PLEXSPACES_NODE_ID:-test-node-${HTTP_PORT}}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"

cleanup() {
  curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "================================================================"
echo "  Order Fulfillment Workflow (Rust WASM)"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node. Start: ./scripts/server.sh${NC}"
  exit 1
fi

echo "Step 1: Deploy"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=temporal-order-fulfillment-rust" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed (HTTP $HTTP_CODE): $RESPONSE${NC}"
  exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
  local id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE:$id/ask?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Fail if actor response embeds a WASM / actor error (nested payload.error or status error).
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
p = d.get("payload", d)
if isinstance(p, str):
    try:
        p = json.loads(p)
    except Exception:
        p = {}
if isinstance(p, dict) and p.get("error"):
    print(f"FAIL [{step}]: payload error: {p.get('error')}", file=sys.stderr)
    sys.exit(1)
sys.exit(0)
PY
  then
    echo -e "${RED}Assertion failed: $step_name${NC}"
    echo "  Raw: $(echo "$json_raw" | head -c 400)"
    exit 1
  fi
}

echo "Step 2: Single order (workflow_run)"
RUN=$(send_op "order-1" '{"op":"workflow_run","order_id":"order-1","customer_id":"cust-1"}' 15)
assert_actor_ok "workflow_run" "$RUN"
if ! echo "$RUN" | grep -q '"status":"shipped"'; then
  echo -e "${RED}Expected shipped status in response${NC}"
  echo "$RUN"
  exit 1
fi
echo -e "  ${GREEN}Order order-1 shipped${NC}"

echo "Step 3: Query status"
QUERY=$(send_op "order-1" '{"op":"workflow_query:status"}' 5)
assert_actor_ok "workflow_query" "$QUERY"
echo "  Query: $(echo "$QUERY" | head -c 200)..."

echo "Step 4: Cancel signal (order-2)"
RUN2=$(send_op "order-2" '{"op":"workflow_run","order_id":"order-2","customer_id":"cust-2"}' 15)
assert_actor_ok "workflow_run order-2" "$RUN2"
SIG=$(send_op "order-2" '{"op":"workflow_signal:cancel"}' 5)
assert_actor_ok "workflow_signal:cancel" "$SIG"
Q2=$(send_op "order-2" '{"op":"workflow_query:status"}' 5)
assert_actor_ok "workflow_query order-2" "$Q2"
if ! echo "$Q2" | grep -q '"cancel_requested":true'; then
  echo -e "${RED}Expected cancel_requested true${NC}"
  echo "$Q2"
  exit 1
fi
echo -e "  ${GREEN}Cancel received${NC}"

echo "Step 5: Batch $NUM_ORDERS orders"
BATCH_START=$(date +%s%N)
for i in $(seq 1 $NUM_ORDERS); do
  R=$(send_op "order-batch-$i" "{\"op\":\"workflow_run\",\"order_id\":\"order-batch-$i\",\"customer_id\":\"cust-$i\"}" 15)
  assert_actor_ok "batch order-batch-$i" "$R"
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Orders/sec: $(echo "scale=1; $NUM_ORDERS * 1000 / $WALL_MS" | bc 2>/dev/null || echo "N/A")${NC}"
echo ""

echo "Step 6: Metrics (actor query + application list)"
echo "================================================================"
STATS_RESP=$(send_op "order-batch-$NUM_ORDERS" '{"op":"workflow_query:status"}' 5)
assert_actor_ok "workflow_query batch sample" "$STATS_RESP"

export WALL_MS NUM_ORDERS APP_ID HTTP_PORT NODE_ID STATS_RESP
if ! STATS_RESP="$STATS_RESP" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("STATS_RESP", "")
d = json.loads(raw)
p = d.get("payload", d)
if isinstance(p, str):
    try:
        p = json.loads(p)
    except Exception:
        p = {}
order_id = p.get("order_id", "N/A")
status = p.get("status", "N/A")
compute_ms = float(p.get("total_compute_ms", 0) or 0)
coord_ms = float(p.get("total_coord_ms", 0) or 0)
steps = p.get("steps_count", 0)
total_ms = compute_ms + coord_ms
if total_ms > 0:
    compute_pct = 100 * compute_ms / total_ms
    coord_pct = 100 * coord_ms / total_ms
    gran = compute_ms / coord_ms if coord_ms > 0 else 0
else:
    compute_pct = coord_pct = gran = 0
wall_ms = int(os.environ.get("WALL_MS", 0))
num_orders = int(os.environ.get("NUM_ORDERS", 1))
orders_per_sec = num_orders / (wall_ms / 1000) if wall_ms > 0 else 0
print()
print("  Order (sample from workflow_query)")
print("  ────────────────────────────────────────────")
print(f"  Order ID:           {order_id}")
print(f"  Status:             {status}")
print(f"  Steps completed:    {steps}")
print()
print("  Benchmarks (coord vs compute)")
print("  ────────────────────────────────────────────")
print(f"  Compute time:       {compute_ms:.1f}ms ({compute_pct:.1f}%)")
print(f"  Coordination time:  {coord_ms:.1f}ms ({coord_pct:.1f}%)")
print(f"  Granularity:        {gran:.1f}x (compute/coordinate)")
print()
print(f"  Batch total wall:   {wall_ms}ms")
print(f"  Orders:             {num_orders}")
print(f"  Orders/sec:         {orders_per_sec:.1f}")
PY
then
  echo -e "${RED}Failed to parse workflow_query metrics${NC}"
  exit 1
fi

APP_LIST=$(fetch_applications_list_json "http://localhost:$HTTP_PORT")
export APP_LIST
APP_LIST="$APP_LIST" APP_ID="$APP_ID" NODE_ID="$NODE_ID" python3 - <<'PY' || true
import json, os, sys, urllib.request
raw = os.environ.get("APP_LIST", "[]")
app_id = os.environ.get("APP_ID", "")
node_id = os.environ.get("NODE_ID", "")
try:
    data = json.loads(raw)
except json.JSONDecodeError:
    print("  (Could not parse GET /api/v1/applications)")
    sys.exit(0)
apps = data.get("applications", data) if isinstance(data, dict) else data
if not isinstance(apps, list):
    apps = []
found = None
for a in apps:
    if not isinstance(a, dict):
        continue
    aid = a.get("application_id") or a.get("name") or ""
    if aid == app_id or app_id in str(aid):
        found = a
        break
if not found:
    print()
    print("  Application metrics (HTTP): app not found in list (ok if API shape differs)")
    sys.exit(0)
m = found.get("metrics") or (found.get("application") or {}).get("metrics")
if m is None:
    print()
    print("  Application metrics (HTTP): no metrics field on list entry")
    sys.exit(0)
cm = m.get("counter_metrics") or {}
print()
print("  Application metrics (merged via host::application_metrics_add)")
print("  ────────────────────────────────────────────")
for k in sorted(cm.keys()):
    if k.startswith("workflow"):
        print(f"  {k}: {cm[k]}")
lat = m.get("latency_totals_ms") or {}
if lat:
    print("  latency_totals_ms (workflow.*):")
    for k in sorted(lat.keys()):
        if "workflow" in k:
            print(f"    {k}: {lat[k]}")
PY

echo ""
echo "================================================================"
echo -e "  ${GREEN}Order Fulfillment Workflow (Rust) Test Complete${NC}"
echo "================================================================"
