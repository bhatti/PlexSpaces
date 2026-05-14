#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/web_crawl_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
export CARGO_PROFILE="${CARGO_PROFILE:-debug}"

if [[ -f "$HOME/venv/bin/activate" ]]; then
  source "$HOME/venv/bin/activate" 2>/dev/null || true
fi

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="web-crawl-rust"
APP_NAME="web-crawl-rust"
ORCHESTRATOR_ACTOR="orchestrator"

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
  for node in "${NODE_LIST[@]}"; do
    curl -s -X DELETE "http://${node%%:*}:${node##*:}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Build did not produce $WASM_FILE${NC}"
  exit 1
fi

# Verify node is reachable
http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${ENTRY_HOST}:${ENTRY_PORT}/" 2>/dev/null) || http_code="000"
if [ "$http_code" = "000" ]; then
  echo -e "${RED}Cannot reach node at ${ENTRY_HOST}:${ENTRY_PORT}${NC}"
  exit 1
fi

TEMP_CONFIG="$(mktemp -t web-crawl-app-config)"
cp "$CONFIG_FILE" "$TEMP_CONFIG"

echo "Step 1: Deploy to ${ENTRY_HOST}:${ENTRY_PORT}"
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

echo "Step 2: Start crawl"
crawl_response=$(curl -s --max-time 60 \
  -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$ORCHESTRATOR_ACTOR/ask?timeout=60" \
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
print(f"  Top words:")
for entry in payload.get("top_words", []):
    if isinstance(entry, list) and len(entry) >= 2:
        print(f"    {entry[0]:<20} {entry[1]}")
if payload.get("pages_crawled", 0) < 1:
    raise SystemExit("Expected at least 1 page crawled")
PY
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Web crawl test passed.${NC}"
