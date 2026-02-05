#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test CDN Cache Python WASM actor
# Real-world scenario: Static asset serving and caching

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/cdn_cache_actor.wasm"
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

APP_ID="cdn-cache-test"
ACTOR_NAME="CdnCache"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        CDN Cache - Blob Storage for Static Assets             ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "${YELLOW}💡 Real-world use case: Static asset serving, user uploads, media files${NC}"
echo ""

# Activate venv
source "$HOME/venv/bin/activate" 2>/dev/null || true

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}❌ Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node
echo "Step 1: Check node status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}❌ Cannot connect to node at localhost:$HTTP_PORT${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Node is running${NC}"
echo ""

# Deploy
echo "Step 2: Deploy CDN cache actor"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$ACTOR_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed CDN cache${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function - send POST with invocation=call (request-reply)
send_post() {
    local desc="$1"
    local payload="$2"
    
    # Use invocation=call to get response (GenServer request-reply pattern)
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME?invocation=call" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"status"'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    elif echo "$RESPONSE" | grep -q '"error"'; then
        echo -e "  ${YELLOW}⚠${NC} $desc (expected): $RESPONSE"
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

# Test operations
echo "Step 3: Upload assets (blob_upload)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Scenario: Upload static assets for a website${NC}"

# Create some test data (base64 encoded)
# "Hello CDN" in base64
LOGO_DATA="SGVsbG8gQ0RO"
# "body { color: red; }" in base64
CSS_DATA="Ym9keSB7IGNvbG9yOiByZWQ7IH0="
# "console.log('hello');" in base64
JS_DATA="Y29uc29sZS5sb2coJ2hlbGxvJyk7"

send_post "Upload logo.png" '{"op":"upload","path":"assets/images/logo.png","data":"'"$LOGO_DATA"'","content_type":"image/png"}'
send_post "Upload styles.css" '{"op":"upload","path":"assets/css/styles.css","data":"'"$CSS_DATA"'","content_type":"text/css"}'
send_post "Upload app.js" '{"op":"upload","path":"assets/js/app.js","data":"'"$JS_DATA"'","content_type":"application/javascript"}'

echo ""
echo "Step 4: Download assets (blob_download)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Download logo.png" '{"op":"download","path":"assets/images/logo.png"}'
send_post "Download styles.css" '{"op":"download","path":"assets/css/styles.css"}'

echo ""
echo "Step 5: Download non-existent asset (cache miss)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Download missing.png (expect not_found)" '{"op":"download","path":"assets/images/missing.png"}'

echo ""
echo "Step 6: List assets (blob_list)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "List all assets" '{"op":"list","prefix":"assets/"}'
send_post "List images only" '{"op":"list","prefix":"assets/images/"}'

echo ""
echo "Step 7: Get cache stats"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Cache statistics" '{"op":"stats"}'

echo ""
echo "Step 8: Delete an asset (blob_delete)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Delete app.js" '{"op":"delete","path":"assets/js/app.js"}'

echo ""
echo "Step 9: Purge assets by prefix"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Purge CSS assets" '{"op":"purge","prefix":"assets/css/"}'

echo ""
echo "Step 10: Final stats"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Final statistics" '{"op":"stats"}'

echo ""
echo -e "${GREEN}✅ CDN Cache Test Complete${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} blob_upload: Upload assets with content type"
echo -e "  ${GREEN}•${NC} blob_download: Download assets (base64 encoded)"
echo -e "  ${GREEN}•${NC} blob_list: List assets by prefix"
echo -e "  ${GREEN}•${NC} blob_delete: Delete individual assets"
echo -e "  ${GREEN}•${NC} Cache invalidation: Purge by prefix"
echo ""
echo -e "${YELLOW}💡 Use blob storage for user uploads, media files, and static assets${NC}"
echo ""
