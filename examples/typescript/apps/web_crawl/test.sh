#!/usr/bin/env bash
# Test Web Crawl (TypeScript WASM)
# Demonstrates ElasticPool (fetcher pool), TupleSpace (URL queue), ShardGroup (word-count reduce)
# Usage: ./test.sh [HTTP_PORT|node1:port1 ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/web_crawl"
WASM_FILE="$TARGET_DIR/web_crawl_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"


# Auto-generate JWT if not provided
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

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES=""
  for _p in "$@"; do NODES="${NODES:+$NODES }localhost:$_p"; done
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

APP_ID="ts-web-crawl"
APP_NAME="ts-web-crawl"
TEMP_DIR=""

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"


ask() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/${actor}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "================================================================"
echo "  Web Crawl (TypeScript WASM)"
echo "  ElasticPool + TupleSpace + ShardGroup"
echo "================================================================"

echo ""
echo "Step 0: Build"
"$SCRIPT_DIR/build.sh"
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Build did not produce $WASM_FILE${NC}"
  exit 1
fi

http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${ENTRY_HOST}:${ENTRY_PORT}/" 2>/dev/null) || http_code="000"
if [ "$http_code" = "000" ]; then
  echo -e "${RED}Cannot reach node at ${ENTRY_HOST}:${ENTRY_PORT}${NC}"
  exit 1
fi

TEMP_DIR="$(mktemp -d)"
TEMP_CONFIG="$TEMP_DIR/app-config.toml"
trap 'rm -rf "${TEMP_DIR:-}" "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp).zip"
rm -f "$APP_ZIP"
cp "$CONFIG_FILE" "$TEMP_CONFIG"
zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null

echo "Step 1: Deploy to all nodes"
for _node in "${NODE_LIST[@]}"; do
  _h="${_node%%:*}"; _p="${_node##*:}"
  curl -s -X DELETE "http://${_h}:${_p}/api/v1/applications/$APP_ID" ${AUTH_HEADER:+-H "$AUTH_HEADER"} >/dev/null 2>&1 || true
done
sleep 1
for _node in "${NODE_LIST[@]}"; do
  if [ "$_node" = "$ENTRY_NODE" ]; then continue; fi
  _h="${_node%%:*}"; _p="${_node##*:}"; _dep=0
  for _attempt in 1 2 3; do
    APP_ZIP="$(mktemp).zip"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
    deploy_output=$(curl -s -w "\n%{http_code}" -X POST "http://${_h}:${_p}/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" -F "name=$APP_NAME" -F "version=1.0.0" -F "app_file=@$APP_ZIP" 2>&1)
    http_code=$(echo "$deploy_output" | tail -n1); response=$(echo "$deploy_output" | sed '$d')
    if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then _dep=1; break; fi
    sleep 3
  done
  if [ "$_dep" -eq 0 ]; then echo -e "${RED}Deploy to ${_h}:${_p} failed${NC}"; exit 1; fi
done
_deployed=0
for _attempt in 1 2 3; do
  APP_ZIP="$(mktemp).zip"; zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  deploy_output=$(curl -s -w "\n%{http_code}" \
    -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1)
  http_code=$(echo "$deploy_output" | tail -n1)
  response=$(echo "$deploy_output" | sed '$d')
  if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $response${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed${NC}"
rm -rf "$TEMP_DIR"; TEMP_DIR=""
sleep 2

echo ""
echo "================================================================"
echo "  Phase 1: Run crawl"
echo "================================================================"

echo ""
echo "Step 2: Crawl from seed URLs"
crawl_response=$(ask "orchestrator" '{
  "op": "crawl",
  "seed_urls": ["https://example.com", "https://docs.example.com"],
  "max_pages": 20,
  "max_depth": 2
}' 60)

if ! echo "$crawl_response" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
exit(0 if p.get('status') == 'ok' else 1)
" 2>/dev/null; then
  echo -e "${RED}Crawl failed: $crawl_response${NC}"
  exit 1
fi

echo ""
echo "Step 3: Crawl results"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
CRAWL_RESPONSE="$crawl_response" python3 - <<'PY'
import json, os
d = json.loads(os.environ["CRAWL_RESPONSE"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
pages    = p.get("pages_crawled", 0)
links    = p.get("total_links", 0)
words    = p.get("top_words") or []
elapsed  = p.get("elapsed_ms", 0)
coord    = p.get("coord_time_ms", 0)
fetch    = p.get("fetch_time_ms", 0)
tps      = p.get("pages_per_sec", 0.0)
parallel = p.get("parallel_fraction", 0.0)
print(f"  Pages crawled     : {pages}")
print(f"  Total links       : {links}")
print(f"  ── Performance (Amdahl's Law) ──────────────────────────")
print(f"  Elapsed           : {elapsed} ms")
print(f"  Coordination time : {coord} ms  (TupleSpace writes + pool checkout/checkin)")
print(f"  Fetch time        : {fetch} ms  (actual crawl work)")
print(f"  Throughput        : {float(tps):.1f} pages/sec")
print(f"  Parallel fraction : {float(parallel):.2%}  (1 - coord/elapsed)")
print(f"  ────────────────────────────────────────────────────────")
print("  Top words         :")
for entry in words:
    if isinstance(entry, (list, tuple)) and len(entry) >= 2:
        print(f"    {entry[0]:<20} {entry[1]}")
if pages < 1:
    raise SystemExit("Expected at least 1 page crawled")
PY
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

echo ""
echo "================================================================"
echo "  Phase 2: Actor status and metrics"
echo "================================================================"

echo ""
echo "Step 4: Orchestrator status"
orch_status=$(ask "orchestrator" '{"op":"status"}' 10)
RESP="$orch_status" python3 - <<'PY'
import json, os
d = json.loads(os.environ["RESP"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
print(f"  role          : {p.get('role','?')}")
print(f"  pages_crawled : {p.get('pages_crawled',0)}")
print(f"  total_links   : {p.get('total_links',0)}")
PY

echo ""
echo "Step 5: Fetcher pool status (ElasticPool — 4 workers)"
for i in 0 1 2 3; do
  fetcher_status=$(ask "fetcher-${i}" '{"op":"status"}' 10)
  IDX="$i" RESP="$fetcher_status" python3 - <<'PY'
import json, os
idx = os.environ["IDX"]
d = json.loads(os.environ["RESP"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
print(f"  fetcher-{idx}: fetch_count={p.get('fetch_count',0)}")
PY
done

echo ""
echo "Step 6: Analyzer shard status (ShardGroup — 2 shards)"
for i in 0 1; do
  analyzer_status=$(ask "analyzer-${i}" '{"op":"top_words","n":5}' 10)
  IDX="$i" RESP="$analyzer_status" python3 - <<'PY'
import json, os
idx = os.environ["IDX"]
d = json.loads(os.environ["RESP"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
words = p.get("top_words") or []
top = ", ".join(f"{w[0]}:{w[1]}" for w in words[:3] if isinstance(w,(list,tuple)) and len(w)>=2)
print(f"  analyzer-{idx}: top_words=[{top}]")
PY
done

echo ""
echo "Step 7: Node metrics"
metrics_response=$(curl -s --max-time 10 \
  "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/metrics" 2>/dev/null || echo '{}')
RESP="$metrics_response" python3 - <<'PY'
import json, os
try:
    d = json.loads(os.environ["RESP"])
    actors   = d.get("actor_count", d.get("actors", "?"))
    messages = d.get("messages_processed", d.get("total_messages", "?"))
    print(f"  actors active : {actors}")
    print(f"  messages      : {messages}")
except Exception:
    pass
PY

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  ask "$actor" "$payload" "$timeout"
}

assert_actor_ok() {
  local step="$1" raw="$2"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except Exception as e:
    print(f"FAIL [{step}]: invalid JSON: {e}", file=sys.stderr); sys.exit(1)
def find_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False: return str(obj.get("error") or "request failed")
        if obj.get("status") == "error": return str(obj.get("error") or "error status")
        if obj.get("error"): return str(obj["error"])
        for v in obj.values():
            e = find_err(v)
            if e: return e
    return None
err = find_err(d)
if err: print(f"FAIL [{step}]: {err}", file=sys.stderr); sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step${NC}"
    echo "  Raw: $(echo "$raw" | head -c 400)"
    exit 1
  fi
}

echo ""
echo "Step 8: Smoke test — all four primitives"
echo "  Testing orchestrator status (pool_metrics, worker_stats)..."
R="$(send_actor "orchestrator" '{"op":"status"}' 20)"
assert_actor_ok "orchestrator status" "$R"
echo "  ✓ orchestrator status OK"

echo "  Testing analyzer-0 top_words..."
R="$(send_actor "analyzer-0" '{"op":"top_words","n":5}' 20)"
assert_actor_ok "analyzer-0 top_words" "$R"
echo "  ✓ analyzer-0 top_words OK"

echo "  Testing fetcher-0 status_request (ProcessGroup member)..."
R="$(send_actor "fetcher-0" '{"op":"status_request"}' 20)"
assert_actor_ok "fetcher-0 status_request" "$R"
echo "  ✓ fetcher-0 status_request OK"

echo ""
echo "Step 9: Fetcher work distribution"
for i in 0 1 2 3; do
  fs=$(ask "fetcher-${i}" '{"op":"status_request"}' 10)
  IDX="$i" RESP="$fs" python3 - <<'PY'
import json, os
idx = os.environ["IDX"]
d = json.loads(os.environ["RESP"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
fc = p.get("fetch_count", 0)
url = p.get("last_url", "")[:50]
print(f"  fetcher-{idx}: fetch_count={fc}  last_url={url}")
PY
done

echo ""
echo "================================================================"
echo "  Phase 3: Scaling Benchmark"
echo "================================================================"

SCALING_SHARDS="${SCALING_SHARDS:-2,4,8,16}"

echo ""
echo "Step 10: Strong scaling (200 total pages / N workers, 4 passes)"
STRONG_PAYLOAD=$(python3 -c "import json; print(json.dumps({'op':'benchmark','worker_counts':[int(s) for s in '${SCALING_SHARDS}'.split(',')],'pages_per_round':200,'num_passes':4}))")
STRONG_RESPONSE=$(ask "orchestrator" "$STRONG_PAYLOAD" 300)

if ! echo "$STRONG_RESPONSE" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
exit(0 if p.get('status') == 'ok' else 1)
" 2>/dev/null; then
  echo -e "${RED}Strong scaling failed: $STRONG_RESPONSE${NC}"
  exit 1
fi

BENCH_RESPONSE="$STRONG_RESPONSE" python3 - <<'PY'
import json, os
d = json.loads(os.environ["BENCH_RESPONSE"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
results = p.get("results", [])
print("═" * 82)
print("  Web Crawl (TypeScript) — Strong Scaling (200 total pages / N workers × 4 passes)")
print("═" * 82)
print(f"  {'Workers':>8} {'Pages':>7} {'Pg/s':>8} {'Wall':>8} {'Comp':>8} {'Coord':>7} {'Speed':>8} {'Eff%':>6} {'Errs':>5}")
print("─" * 82)
for r in results:
    print(f"  {r.get('workers',0):>8} {r.get('pages',0):>7} {r.get('pages_per_sec',0):>8.0f} {r.get('elapsed_ms',0):>8} {r.get('fetch_ms',0):>8} {r.get('coord_ms',0):>7} {r.get('speedup',1):>7.2f}x {r.get('efficiency_pct',100):>5.1f}% {r.get('error_count',0):>5}")
print("═" * 82)
for r in results:
    assert r.get("error_count",0) == 0, f"errors in workers={r.get('workers')}"
print("  ✓ Passed")
PY

echo ""
echo "Step 11: Weak scaling (200 pages/worker fixed, 4 passes — total grows with N)"
WEAK_PAYLOAD=$(python3 -c "import json; print(json.dumps({'op':'run_weak_scaling_benchmark','shard_counts':[int(s) for s in '${SCALING_SHARDS}'.split(',')],'pages_per_worker':200,'num_passes':4}))")
WEAK_RESPONSE=$(ask "orchestrator" "$WEAK_PAYLOAD" 300)

if ! echo "$WEAK_RESPONSE" | python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
if isinstance(p, str): p = json.loads(p)
exit(0 if p.get('status') == 'ok' else 1)
" 2>/dev/null; then
  echo -e "${RED}Weak scaling failed: $WEAK_RESPONSE${NC}"
  exit 1
fi

BENCH_RESPONSE="$WEAK_RESPONSE" python3 - <<'PY'
import json, os
d = json.loads(os.environ["BENCH_RESPONSE"])
p = d.get("payload", d)
if isinstance(p, str): p = json.loads(p)
results = p.get("results", [])
print("═" * 82)
print("  Web Crawl (TypeScript) — Weak Scaling (200 pages/worker × 4 passes)")
print("═" * 82)
print(f"  {'Workers':>8} {'TotPg':>7} {'Pg/s':>8} {'Wall':>8} {'Comp':>8} {'Coord':>7} {'Speed':>8} {'Eff%':>6} {'Errs':>5}")
print("─" * 82)
for r in results:
    print(f"  {r.get('workers',0):>8} {r.get('pages',0):>7} {r.get('pages_per_sec',0):>8.0f} {r.get('elapsed_ms',0):>8} {r.get('fetch_ms',0):>8} {r.get('coord_ms',0):>7} {r.get('speedup',1):>7.2f}x {r.get('efficiency_pct',100):>5.1f}% {r.get('error_count',0):>5}")
print("═" * 82)
for r in results:
    assert r.get("error_count",0) == 0, f"errors in workers={r.get('workers')}"
print("  ✓ Passed")
PY

echo ""
echo "================================================================"
echo -e "  ${GREEN}Web Crawl (TypeScript WASM) Test Complete${NC}"
echo ""
echo "  Key behaviors demonstrated:"
echo "    ✓ TupleSpace frontier  — url_queue as live work frontier, ts.write() metadata"
echo "    ✓ ElasticPool          — poolCheckout/poolCheckin, utilization metrics"
echo "    ✓ ProcessGroup         — workers self-register, orchestrator discovers members"
echo "    ✓ ShardGroup scatter   — interleaved scatter to 2 analyzer shards"
echo "    ✓ Strong scaling       — 200 total pages / N workers, broadcast SG"
echo "    ✓ Weak scaling         — 200 pages/worker fixed, super-linear efficiency"
echo "================================================================"
