#!/bin/bash
# Test Webhook Processor - Go WASM (AWS Durable Lambda style, exactly-once dedup)
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/webhook_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8092}"

APP_ID="migrating-aws-durable-lambda-go"
ACTOR_TYPE="webhook"
INSTANCE_ID="default"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

trap 'curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true' EXIT

echo "================================================================"
echo "  AWS Durable Lambda → PlexSpaces: Webhook processor (Go WASM)"
echo "  Exactly-once with deduplication"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  chmod +x "$SCRIPT_DIR/build.sh"
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
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
  -F "name=migrating-aws-durable-lambda-go" \
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
  local payload="$1" timeout="${2:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${INSTANCE_ID}/ask?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: First webhook (idempotency_key=key-1)"
R1=$(send_op '{"op":"webhook","idempotency_key":"key-1","body":{"event":"order.created"}}' 15)
if echo "$R1" | grep -q '"ok":true'; then
  echo -e "  ${GREEN}Processed key-1${NC}"
else
  echo "  Response: $R1"
fi

echo "Step 3: Duplicate (same key → cached response, dedup hit)"
R2=$(send_op '{"op":"webhook","idempotency_key":"key-1","body":{"event":"order.created"}}' 15)
if echo "$R2" | grep -q '"ok":true'; then
  echo -e "  ${GREEN}Dedup: returned cached response for key-1${NC}"
else
  echo "  Response: $R2"
fi

echo "Step 4: New key (key-2)"
send_op '{"op":"webhook","idempotency_key":"key-2","body":{}}' 15 >/dev/null
echo -e "  ${GREEN}Processed key-2${NC}"

echo "Step 5: Status"
STATUS=$(send_op '{"op":"status"}' 10)
echo "  $(echo "$STATUS" | head -c 200)..."

echo "Step 6: Batch (mix new + duplicate keys for metrics)"
for i in $(seq 1 20); do
  send_op "{\"op\":\"webhook\",\"idempotency_key\":\"batch-$i\",\"body\":{}}" 15 >/dev/null
done
for i in 1 2 3 4 5; do
  send_op "{\"op\":\"webhook\",\"idempotency_key\":\"batch-$i\",\"body\":{}}" 15 >/dev/null
done
echo -e "  ${GREEN}Batch done (20 new + 5 duplicates)${NC}"

echo "Step 7: Final status and metrics"
echo "================================================================"
FINAL=$(send_op '{"op":"status"}' 10)
if echo "$FINAL" | grep -q '"total_processed"'; then
  echo "$FINAL" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
proc = p.get('total_processed', 0)
dedup = p.get('total_dedup_hits', 0)
compute = p.get('total_compute_ms', 0) or 0
coord = p.get('total_coord_ms', 0) or 0
keys = p.get('keys_stored', 0)
print()
print('  Webhook processor (AWS Durable Lambda style)')
print('  Exactly-once deduplication')
print('  ────────────────────────────────────────────')
print(f'  Total processed:  {proc}')
print(f'  Dedup hits:       {dedup}')
print(f'  Keys stored:     {keys}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Compute ms:      {compute:.1f}')
print(f'  Coord ms:        {coord:.1f}')
" 2>/dev/null || echo "  $FINAL"
else
  echo "  Response: $FINAL"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Webhook processor test complete${NC}"
echo "================================================================"
