#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Receipt Storage Python WASM actor deployment and execution
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/receipt_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="receipt-storage-test"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Receipt Storage Test (Python WASM)                      ║"
echo "╚════════════════════════════════════════════════════════════════╝"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 1: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
  -F "application_id=$APP_ID" \
  -F "name=$APP_ID" \
  -F "version=1.0.0" \
  -F "wasm_file=@$WASM_FILE;type=application/wasm" \
  -F "config=@$CONFIG_FILE" 2>&1)
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed $APP_ID${NC}"
sleep 2

ask() {
  local payload="$1"
  curl -s --max-time 15 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/receipt_store:receipt_storage/ask?timeout=15" \
    -H "Content-Type: application/json" \
    -d "$payload" 2>/dev/null || echo '{"status":"error","error":"timeout"}'
}

echo "Step 2: Test receipt operations"
STORE1=$(ask '{"op":"store","merchant":"Starbucks","amount":5.75,"date":"2024-01-15","description":"Grande latte"}')
STORE2=$(ask '{"op":"store","merchant":"Whole Foods","amount":87.32,"date":"2024-01-15","description":"Weekly groceries"}')
STORE3=$(ask '{"op":"store","merchant":"Shell","amount":45.00,"date":"2024-01-15","description":"Gas fill-up"}')
LIST=$(ask '{"op":"list"}')
SUMMARY=$(ask '{"op":"summary"}')

echo ""
STORE1="$STORE1" STORE2="$STORE2" STORE3="$STORE3" LIST="$LIST" SUMMARY="$SUMMARY" python3 - <<'PY'
import json, os

def payload(raw):
    top = json.loads((raw or "").strip() or "{}")
    body = top.get("payload", top)
    if isinstance(body, str):
        body = json.loads(body.strip() or "{}")
    return body if isinstance(body, dict) else {}

s1 = payload(os.environ["STORE1"])
s2 = payload(os.environ["STORE2"])
s3 = payload(os.environ["STORE3"])
lst = payload(os.environ["LIST"])
summ = payload(os.environ["SUMMARY"])

print(f"  store Starbucks:  {s1.get('status', s1)}")
print(f"  store Whole Foods:{s2.get('status', s2)}")
print(f"  store Shell:      {s3.get('status', s3)}")
print(f"  list count:       {len(lst.get('receipts', []))}")
print(f"  total spending:   {summ.get('total', summ.get('total_amount', '?'))}")
PY

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo -e "║  ${GREEN}Receipt Storage (Python) example test complete${NC}               ║"
echo "╚════════════════════════════════════════════════════════════════╝"
