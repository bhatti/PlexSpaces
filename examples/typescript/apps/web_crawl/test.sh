#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/web_crawl"
WASM_FILE="$TARGET_DIR/web_crawl_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="web-crawl-typescript"
APP_NAME="web-crawl-typescript"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"
TEMP_CONFIG=""

cleanup() {
  [[ -n "${TEMP_CONFIG:-}" && -f "${TEMP_CONFIG:-}" ]] && rm -f "$TEMP_CONFIG"
  curl -s -X DELETE "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

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

TEMP_CONFIG="$(mktemp -t web-crawl-ts-config)"
cp "$CONFIG_FILE" "$TEMP_CONFIG"

echo "Step 1: Deploy"
curl -s -X DELETE "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
deploy_output=$(curl -s --connect-timeout 10 --max-time 60 -w "\n%{http_code}" \
  -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_NAME" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$TEMP_CONFIG" 2>&1)
http_code=$(echo "$deploy_output" | tail -n1)
response=$(echo "$deploy_output" | sed '$d')
if [ "$http_code" != "200" ] || ! echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
  echo -e "${RED}Deploy failed: $response${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed${NC}"
rm -f "$TEMP_CONFIG"; TEMP_CONFIG=""
sleep 2

echo "Step 2: Run crawl"
crawl_response=$(curl -s --max-time 60 \
  -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/orchestrator/ask?timeout=60" \
  -H "Content-Type: application/json" \
  -d '{"op":"crawl","seed_urls":["https://example.com","https://docs.example.com"],"max_pages":20,"max_depth":2}')

if ! echo "$crawl_response" | grep -q '"status":"ok"'; then
  echo -e "${RED}Crawl failed: $crawl_response${NC}"
  exit 1
fi

echo "Step 3: Results"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
python3 - "$crawl_response" <<'PY'
import json, sys
data = json.loads(sys.argv[1])
payload = data.get("payload", data)
if isinstance(payload, str):
    payload = json.loads(payload)
print(f"  Pages crawled : {payload.get('pages_crawled', 0)}")
print(f"  Total links   : {payload.get('total_links', 0)}")
print("  Top words:")
for entry in (payload.get("top_words") or []):
    if isinstance(entry, (list, tuple)) and len(entry) >= 2:
        print(f"    {entry[0]:<20} {entry[1]}")
if payload.get("pages_crawled", 0) < 1:
    raise SystemExit("Expected at least 1 page crawled")
PY
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Web crawl (TypeScript) test passed.${NC}"
