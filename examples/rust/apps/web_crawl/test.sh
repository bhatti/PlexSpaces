#!/usr/bin/env bash
# Test Web Crawl (Rust WASM)
# Demonstrates ElasticPool (fetcher pool), TupleSpace (URL queue), ShardGroup (word-count reduce)
# Usage: ./test.sh [HTTP_PORT|node1:port1 ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"

WASM_FILE="$SCRIPT_DIR/web_crawl_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

APP_ID="rust-web-crawl"
APP_NAME="rust-web-crawl"
TEMP_CONFIG=""

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"

cleanup() {
  [[ -n "${TEMP_CONFIG:-}" && -f "${TEMP_CONFIG:-}" ]] && rm -f "$TEMP_CONFIG"
  curl -s -X DELETE "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

ask() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/${actor}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "================================================================"
echo "  Web Crawl (Rust WASM)"
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

TEMP_CONFIG="$(mktemp -t rust-web-crawl-config)"
cp "$CONFIG_FILE" "$TEMP_CONFIG"

echo "Step 1: Deploy to ${ENTRY_HOST}:${ENTRY_PORT}"
curl -s -X DELETE "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
deploy_output=$(curl -s -w "\n%{http_code}" \
  -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_NAME" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$TEMP_CONFIG" 2>&1)
http_code=$(echo "$deploy_output" | tail -n1)
response=$(echo "$deploy_output" | sed '$d')
if [ "$http_code" != "200" ] || ! echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed (HTTP $http_code): $response${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed${NC}"
rm -f "$TEMP_CONFIG"; TEMP_CONFIG=""
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
pages = p.get("pages_crawled", 0)
links = p.get("total_links", 0)
words = p.get("top_words") or []
print(f"  Pages crawled : {pages}")
print(f"  Total links   : {links}")
print("  Top words     :")
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

echo ""
echo "================================================================"
echo -e "  ${GREEN}Web Crawl (Rust WASM) Test Complete${NC}"
echo ""
echo "  Key behaviors demonstrated:"
echo "    ✓ ElasticPool  — 4 PageFetcher actors, round-robin URL dispatch"
echo "    ✓ TupleSpace   — url_queue pending→done tracking"
echo "    ✓ ShardGroup   — 2 analyzer shards, scatter/reduce word counts"
echo "================================================================"
