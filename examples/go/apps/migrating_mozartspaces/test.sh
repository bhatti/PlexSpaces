#!/bin/bash
# Test Distributed Auction - MozartSpaces style (Go WASM)
# TupleSpace (bids) + process group (broadcast) + lock (commit).
# Usage: ./test.sh [HTTP_PORT]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/auction_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

APP_ID="migrating-mozartspaces-auction-go"
ACTOR_TYPE="auction"
AUCTION_ID="auc-1"
NUM_BIDS=150
RESERVE=10

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'



# Auto-generate JWT if not provided
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
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

echo "================================================================"
echo "  MozartSpaces → PlexSpaces: Distributed Auction (Go WASM)"
echo "  TupleSpace + process group + lock"
echo "================================================================"

if [ ! -f "$WASM_FILE" ]; then
  echo "Building WASM..."
  "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
  echo ""
fi

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node: ./scripts/server.sh (from repo root)${NC}"
  exit 1
fi

echo "Step 1: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=migrating-mozartspaces-auction-go" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
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
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

send_op() {
  local instance_id="$1" payload="$2" timeout="${3:-60}"
  curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${instance_id}/ask?timeout=$timeout" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "Step 2: Inject bids (place_bid) into tuple space"
for i in $(seq 1 "$NUM_BIDS"); do
  amt=$((RESERVE + i))
  send_op "coord-1" "{\"op\":\"place_bid\",\"auction_id\":\"$AUCTION_ID\",\"bidder_id\":\"bidder-$i\",\"amount\":$amt}" 5 >/dev/null
done
echo -e "  ${GREEN}Injected $NUM_BIDS bids${NC}"

echo "Step 3: Run auction (coordinator gathers bids, lock, commit, broadcast)"
RUN1=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"auction_id\":\"$AUCTION_ID\",\"reserve_price\":$RESERVE,\"max_bids\":$NUM_BIDS}" 45)
if echo "$RUN1" | grep -q '"status":"sold"'; then
  echo -e "  ${GREEN}Auction $AUCTION_ID sold${NC}"
elif echo "$RUN1" | grep -q '"winner_id"'; then
  echo -e "  ${GREEN}Auction $AUCTION_ID completed${NC}"
else
  echo "  Response: $(echo "$RUN1" | head -c 220)..."
fi

echo "Step 4: Query status"
send_op "coord-1" '{"op":"workflow_query:status"}' 5 | head -c 160
echo "..."

echo "Step 5: Second auction (fewer bids)"
AUCTION_ID2="auc-2"
for i in $(seq 1 20); do
  amt=$((50 + i))
  send_op "coord-1" "{\"op\":\"place_bid\",\"auction_id\":\"$AUCTION_ID2\",\"bidder_id\":\"b-$i\",\"amount\":$amt}" 5 >/dev/null
done
RUN2=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"auction_id\":\"$AUCTION_ID2\",\"reserve_price\":50,\"max_bids\":20}" 30)
if echo "$RUN2" | grep -q '"bids_processed"'; then
  echo -e "  ${GREEN}Auction $AUCTION_ID2 run done${NC}"
fi

echo "Step 6: Batch 5 auctions"
BATCH_START=$(date +%s%N)
LAST_RUN=""
for i in $(seq 1 5); do
  AID="batch-$i"
  for b in $(seq 1 30); do
    send_op "coord-1" "{\"op\":\"place_bid\",\"auction_id\":\"$AID\",\"bidder_id\":\"u-$b\",\"amount\":$((100 + b + i))}" 5 >/dev/null
  done
  RESP=$(send_op "coord-1" "{\"op\":\"workflow_run\",\"auction_id\":\"$AID\",\"reserve_price\":100,\"max_bids\":30}" 30)
  if [ "$i" -eq 5 ]; then LAST_RUN="$RESP"; fi
done
BATCH_END=$(date +%s%N)
WALL_MS=$(( (BATCH_END - BATCH_START) / 1000000 ))
echo -e "  ${GREEN}Batch done. Wall: ${WALL_MS}ms, Auctions: 5${NC}"
echo ""

echo "Step 7: Metrics from last run"
echo "================================================================"
if [ -n "$LAST_RUN" ] && echo "$LAST_RUN" | grep -q '"auction_id"'; then
  export WALL_MS
  echo "$LAST_RUN" | python3 -c "
import sys, json, os
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
  try: p = json.loads(p)
  except: p = {}
aid = p.get('auction_id', 'N/A')
status = p.get('status', 'N/A')
bids = p.get('bids_processed', 0)
winner = p.get('winner_id', 'N/A')
amount = p.get('winning_amount', 0)
compute_ms = p.get('total_compute_ms', 0) or 0
coord_ms = p.get('total_coord_ms', 0) or 0
total_ms = compute_ms + coord_ms
compute_pct = 100 * compute_ms / total_ms if total_ms > 0 else 0
coord_pct = 100 * coord_ms / total_ms if total_ms > 0 else 0
wall_ms = int(os.environ.get('WALL_MS', 0))
print()
print('  Distributed Auction (MozartSpaces-style)')
print('  TupleSpace + process group + lock')
print('  ────────────────────────────────────────────')
print(f'  Auction ID:      {aid}')
print(f'  Status:         {status}')
print(f'  Bids processed: {bids}')
print(f'  Winner:         {winner} @ {amount}')
print()
print('  Benchmarks')
print('  ────────────────────────────────────────────')
print(f'  Compute ms:     {compute_ms:.1f} ({compute_pct:.1f}%)')
print(f'  Coord ms:       {coord_ms:.1f} ({coord_pct:.1f}%)')
print(f'  Batch wall:     {wall_ms}ms')
" 2>/dev/null || echo "  (Parse: $LAST_RUN)"
else
  echo "  Response: $LAST_RUN"
fi

echo ""
echo "================================================================"
echo -e "  ${GREEN}Distributed Auction (MozartSpaces) Test Complete${NC}"
echo "================================================================"
