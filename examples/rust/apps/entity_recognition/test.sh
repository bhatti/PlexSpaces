#!/usr/bin/env bash
# Entity recognition (Rust WASM). process_document, batch, get_status, get_metrics.
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/entity_recognition_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="entity-recognition-wasm-rust"
ACTOR_TYPE="entity_extractor"
INSTANCE_ID="default"

_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "\033[0;31mBuild failed\033[0m"; exit 1; }
echo ""

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  Entity recognition (Rust WASM, SDK + WIT)"
echo "================================================================"

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
  -F "name=entity-recognition-wasm-rust" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
  local payload="$1"
  local timeout="${2:-15}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${INSTANCE_ID}/ask?timeout=$timeout" \
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

DOC_JSON='{"op":"process_document","doc_id":"doc-1","content":"Contact alice@example.com and visit https://example.org for Info."}'

echo "Step 2: process_document"
R1=$(send_op "$DOC_JSON" 20)
assert_actor_ok "process_document" "$R1"
echo "$R1" | grep -q '"entity_count"' || { echo "$R1"; exit 1; }
echo -e "  ${GREEN}process_document OK${NC}"

echo "Step 3: batch"
BATCH_START=$(date +%s%N)
DOCS='['
for i in $(seq 1 15); do
  [ "$i" -gt 1 ] && DOCS+=','
  DOCS+="{\"doc_id\":\"b$i\",\"content\":\"user$i@mail.test https://t$i.example.com\"}"
done
DOCS+=']'
BURST=$(send_op "{\"op\":\"batch\",\"documents\":$DOCS}" 90)
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
assert_actor_ok "batch" "$BURST"
echo -e "  ${GREEN}Batch wall: ${WALL_MS}ms${NC}"

echo "Step 4: Metrics"
METRICS=$(send_op '{"op":"get_status"}' 15)
assert_actor_ok "get_status" "$METRICS"
APP_METRICS=$(send_op '{"op":"get_metrics"}' 15)
assert_actor_ok "get_metrics" "$APP_METRICS"

if ! METRICS_JSON="$METRICS" APP_METRICS_JSON="$APP_METRICS" WALL_MS="$WALL_MS" APP_ID="$APP_ID" HTTP_PORT="$HTTP_PORT" python3 - <<'PY'
import json
import os
import sys
from typing import Any, Dict


def unwrap_actor_payload(raw: str) -> Dict[str, Any]:
    raw = raw.strip()
    if not raw:
        raise ValueError("empty actor HTTP body")
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


def print_metric_maps(m: Dict[str, Any], prefix: str) -> None:
    for key in ("counter_metrics", "latency_totals_ms", "latency_max_ms", "latency_samples"):
        sub = m.get(key)
        if not isinstance(sub, dict):
            continue
        rows = [(k, v) for k, v in sub.items() if prefix in str(k)]
        if not rows:
            continue
        print(f"  {key} ({prefix}*)")
        for k, v in sorted(rows):
            print(f"    {k}: {v}")


raw_metrics = os.environ.get("METRICS_JSON", "")
try:
    p = unwrap_actor_payload(raw_metrics)
except (json.JSONDecodeError, ValueError) as e:
    print(f"ERROR: could not parse get_status response: {e}", file=sys.stderr)
    sys.exit(1)

if not p:
    print("ERROR: empty payload after unwrap", file=sys.stderr)
    sys.exit(1)

compute_ms = float(p.get("total_compute_ms", 0) or 0)
coord_ms = float(p.get("total_coord_ms", 0) or 0)
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
gran = p.get("granularity_ratio")
if gran is None and coord_ms > 0:
    gran = compute_ms / coord_ms
else:
    gran = float(gran or 0)
docs = p.get("documents_processed", 0)
ents = p.get("entities_total", 0)
wall_ms = int(os.environ.get("WALL_MS", 0))

print()
print("  Actor state (get_status payload)")
print("  ────────────────────────────────────────────")
print(f"  documents_processed: {docs}  entities_total: {ents}")
print()
print("  Benchmarks (rollups in actor state)")
print("  ────────────────────────────────────────────")
print(f"  Compute ms:       {compute_ms:.1f} ({compute_pct:.1f}%)")
print(f"  Coord ms:         {coord_ms:.1f} ({coord_pct:.1f}%)")
print(f"  Granularity:      {gran:.1f}x (compute/coordinate)")
print(f"  Batch wall (step 3): {wall_ms}ms")

app_id = os.environ.get("APP_ID", "")
try:
    m = unwrap_actor_payload(os.environ.get("APP_METRICS_JSON", ""))
except (json.JSONDecodeError, ValueError) as e:
    print()
    print(f"  Application metrics: could not parse get_metrics response: {e}")
    sys.exit(0)

print()
print("  Application metrics (host::application_get_metrics)")
print("  ────────────────────────────────────────────")
if not m:
    print("  (no metrics payload)")
    sys.exit(0)

mc = m.get("message_count")
ec = m.get("error_count")
if mc is not None or ec is not None:
    print(f"  message_count: {mc}  error_count: {ec}")

cm = m.get("counter_metrics")
if isinstance(cm, dict) and cm:
    print("  counter_metrics (all keys)")
    for k in sorted(cm.keys()):
        print(f"    {k}: {cm[k]}")

print_metric_maps(m, "entity_recognition")
PY
then
  echo -e "${RED}Step 4 metrics report failed.${NC}"
  exit 1
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Entity recognition (Rust) test complete${NC}"
echo "================================================================"
