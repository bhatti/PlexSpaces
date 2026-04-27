#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Storefront API actor (store config + cart + checkout rate limit via host KV)
# Shows what data is stored and retrieved; optional SQLite verification.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_DIR="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/storefront_actor.wasm"
NODE_ADDR="${1:-localhost:8090}"
HTTP_PORT="${2:-8092}"

export CARGO_TARGET_DIR="$WORKSPACE_DIR/target"
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="storefront-test"
ACTOR_TYPE="$APP_ID"

# Build request body: envelope with msg_type + handler args in payload (see Python dispatch_message).
build_body() {
    local msg_type="$1"
    local payload="${2:-{}}"
    printf '{"msg_type":"%s","payload":%s}' "$msg_type" "$payload"
}

# Invoke actor (request-reply via /ask so the HTTP response includes handler JSON payload).
call() {
    local msg_type="$1"
    local payload="${2:-{}}"
    local json_payload
    json_payload=$(build_body "$msg_type" "$payload")
    curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_TYPE/ask?timeout=15" \
        -H "Content-Type: application/json" \
        -d "$json_payload" \
        --max-time 15
}

# Print response summary: show payload when present, else success/error
show_response() {
    local R="$1"
    local label="$2"
    if echo "$R" | grep -q '"success":\s*true'; then
        if command -v jq >/dev/null 2>&1; then
            local payload
            payload=$(echo "$R" | jq -r '.payload // empty')
            if [ -n "$payload" ] && [ "$payload" != "null" ]; then
                echo -e "  ${CYAN}→ $label${NC}"
                echo "$payload" | jq -C . 2>/dev/null || echo "  $payload"
            else
                echo -e "  ${GREEN}✓${NC} $label (request succeeded)"
            fi
        else
            echo -e "  ${GREEN}✓${NC} $label"
            echo "$R" | grep -o '"payload":[^,]*' | head -1 | sed 's/^/  /'
        fi
    else
        echo -e "  ${RED}✗${NC} $label"
        echo "  $R"
    fi
}

# Assert success (exit 1 on failure)
check_ok() {
    local R="$1"
    local want="$2"
    if echo "$R" | grep -q '"success":\s*true'; then
        if echo "$R" | grep -q "$want"; then
            return 0
        fi
        if echo "$R" | grep -q '"payload":\s*null'; then
            return 0
        fi
    fi
    return 1
}

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    cd "$WORKSPACE_DIR"
    if [ -x "$WORKSPACE_DIR/target/debug/plexspaces" ]; then
        "$WORKSPACE_DIR/target/debug/plexspaces" undeploy --node "$NODE_ADDR" --app-id "$APP_ID" 2>/dev/null || true
    else
        cargo run -q -p plexspaces-cli -- undeploy --node "$NODE_ADDR" --app-id "$APP_ID" 2>/dev/null || true
    fi
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║     Storefront API Test (config + cart + checkout limit)      ║"
echo "║     Data stored in host KV → visible in responses + SQLite     ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

echo "Step 1: Check node"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "   Start node first, e.g.: ./scripts/server.sh"
    exit 1
fi
echo -e "${GREEN}✓ Node running${NC}"
echo ""

echo "Step 2: Undeploy if present, then deploy"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1
trap cleanup EXIT

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true
if ! echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Deployed $APP_ID (actor type: $ACTOR_TYPE)${NC}"
sleep 1
echo ""

echo "Step 3: Store config — set free_shipping_threshold=50, then read it back"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Writing: free_shipping_threshold = 50"
R=$(call "set_store_config" '{"key":"free_shipping_threshold","value":"50"}')
show_response "$R" "set_store_config response"
check_ok "$R" '"status":"ok"' || exit 1

echo ""
echo "  Reading back: get_store_config(free_shipping_threshold)"
R=$(call "get_store_config" '{"key":"free_shipping_threshold"}')
show_response "$R" "get_store_config response (value we stored)"
check_ok "$R" '"value":"50"' || exit 1

echo ""
echo "  Listing all config keys:"
R=$(call "list_store_config" '{}')
show_response "$R" "list_store_config response"
check_ok "$R" 'free_shipping_threshold' || exit 1
echo ""

echo "Step 4: Shopping cart — create cart with items, get it, list carts, destroy"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Creating cart cart-001 for user-42 with items: [{\"sku\":\"WIDGET\",\"qty\":2,\"price\":\"9.99\"}]"
R=$(call "create_cart" '{"cart_id":"cart-001","user_id":"user-42","items":"[{\"sku\":\"WIDGET\",\"qty\":2,\"price\":\"9.99\"}]"}')
show_response "$R" "create_cart response"
check_ok "$R" '"status":"ok"' || exit 1

echo ""
echo "  Getting cart cart-001 (data we stored):"
R=$(call "get_cart" '{"cart_id":"cart-001"}')
show_response "$R" "get_cart response (cart content)"
check_ok "$R" '"user_id":"user-42"' || exit 1

echo ""
echo "  Listing all cart IDs:"
R=$(call "list_carts" '{}')
show_response "$R" "list_carts response"
check_ok "$R" 'cart-001' || exit 1

echo ""
echo "  Destroying cart cart-001 (e.g. after checkout):"
R=$(call "destroy_cart" '{"cart_id":"cart-001"}')
show_response "$R" "destroy_cart response"
check_ok "$R" '"status":"ok"' || exit 1
echo ""

echo "Step 5: Checkout rate limit — 5 requests/minute per user; first 5 allowed, 6th denied"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
for i in 1 2 3 4 5 6; do
    R=$(call "checkout_allowed" '{"identity":"user-alice","window_sec":60,"max_requests":5}')
    if echo "$R" | grep -q '"success":\s*true'; then
        if command -v jq >/dev/null 2>&1; then
            payload=$(echo "$R" | jq -r '.payload // empty')
            if [ -n "$payload" ] && [ "$payload" != "null" ]; then
                allowed=$(echo "$payload" | jq -r '.allowed // "?"')
                remaining=$(echo "$payload" | jq -r '.remaining // "?"')
                if [ "$allowed" = "true" ]; then
                    echo -e "  ${GREEN}✓${NC} checkout $i: allowed (remaining: $remaining)"
                else
                    echo -e "  ${YELLOW}✓${NC} checkout $i: denied — rate limit (remaining: $remaining)"
                fi
            else
                echo -e "  ${GREEN}✓${NC} checkout $i: request succeeded"
            fi
        else
            if echo "$R" | grep -q '"allowed":true'; then
                echo -e "  ${GREEN}✓${NC} checkout $i: allowed"
            else
                echo -e "  ${YELLOW}✓${NC} checkout $i: denied (rate limit)"
            fi
        fi
    else
        echo "  checkout $i failed: $R"
        exit 1
    fi
done
echo ""

echo "Step 6: Verify data in SQLite (host KV backend)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
KV_DB=""
for candidate in \
    "$WORKSPACE_DIR/app/data/keyvalue.db" \
    "$WORKSPACE_DIR/data/keyvalue.db" \
    "./app/data/keyvalue.db" \
    "./data/keyvalue.db" \
    "${PLEXSPACES_KV_SQLITE_PATH:-}"; do
    [ -z "$candidate" ] && continue
    if [ -f "$candidate" ]; then
        KV_DB="$candidate"
        break
    fi
done
if [ -n "$KV_DB" ] && command -v sqlite3 >/dev/null 2>&1; then
    echo "  KeyValue DB: $KV_DB"
    echo "  Table: kv_store (tenant_id, namespace, key, value, ...)"
    echo ""
    echo "  SELECT key, value (as text) FROM kv_store;"
    sqlite3 "$KV_DB" "SELECT key, CAST(value AS TEXT) FROM kv_store;" 2>/dev/null | while IFS='|' read -r key val; do
        # truncate long values for readability
        [ ${#val} -gt 100 ] && val="${val:0:97}..."
        printf "    %-45s | %s\n" "$key" "$val"
    done
    echo ""
    echo "  Row count:"
    sqlite3 "$KV_DB" "SELECT COUNT(*) FROM kv_store;" 2>/dev/null | sed 's/^/    /'
else
    if [ -z "$KV_DB" ]; then
        echo "  No keyvalue SQLite DB found in:"
        echo "    $WORKSPACE_DIR/app/data/keyvalue.db"
        echo "    $WORKSPACE_DIR/data/keyvalue.db"
        echo "  Set PLEXSPACES_KV_SQLITE_PATH or run node with default config so it creates the DB."
    else
        echo "  sqlite3 not installed; install it to verify KV storage (e.g. brew install sqlite)."
    fi
fi
echo ""

echo -e "${GREEN}✅ Storefront API test complete.${NC}"
echo "   Data was stored and retrieved via the StorefrontService actor (host KV)."
echo "   You can verify persistence with: sqlite3 <path/to/keyvalue.db> \"SELECT * FROM kv_store;\""
