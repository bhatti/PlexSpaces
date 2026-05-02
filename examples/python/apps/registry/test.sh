#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Test Registry - Service Discovery with RegistryFacet

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/registry_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8091
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

APP_ID="registry-test"
ACTOR_NAME="registry"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Service Registry - RegistryFacet Integration Test       ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "${YELLOW}💡 Real-world use case: Microservices service discovery${NC}"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "📦 Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
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
echo "Step 2: Deploy service registry with RegistryFacet"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
trap cleanup EXIT

curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
sleep 1

RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1) || true

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}✓ Deployed service registry with RegistryFacet${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function - send POST (fire-and-forget)
send_post() {
    local desc="$1"
    local payload="$2"
    
    RESPONSE=$(curl -s --max-time 10 -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
        -H "Content-Type: application/json" \
        -d "$payload" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

# Helper function - send GET (returns payload)
send_get() {
    local desc="$1"
    local query="${2:-}"
    
    URL="http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME"
    if [ -n "$query" ]; then
        URL="$URL?$query"
    fi
    
    RESPONSE=$(curl -s --max-time 10 -X GET "$URL" 2>/dev/null) || RESPONSE=""
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        echo -e "  ${GREEN}✓${NC} $desc"
    else
        echo -e "  ${YELLOW}⚠${NC} $desc: $RESPONSE"
    fi
}

echo "Step 3: Register Services"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Real-world scenario: Services register themselves${NC}"
echo ""

send_post "Register payment service" '{"msg_type":"register_object","payload":{"object_id":"payment-service-1","object_type":"Service","object_category":"payment","grpc_address":"http://payment-service:50051","capabilities":["process_payment","refund"],"labels":["production","us-east"],"health_status":"Healthy"}}'
send_post "Register user service" '{"msg_type":"register_object","payload":{"object_id":"user-service-1","object_type":"Service","object_category":"user","grpc_address":"http://user-service:50052","capabilities":["get_user","create_user"],"labels":["production","us-west"],"health_status":"Healthy"}}'
send_post "Register order service" '{"msg_type":"register_object","payload":{"object_id":"order-service-1","object_type":"Service","object_category":"order","grpc_address":"http://order-service:50053","capabilities":["create_order","get_order"],"labels":["production","us-east"],"health_status":"Healthy"}}'
echo ""

echo "Step 4: Lookup Services (via GET)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_get "Lookup payment service" "msg_type=lookup_object&object_id=payment-service-1&object_type=Service"
echo ""

echo "Step 5: Discover Services by Type (via GET)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_get "Discover all services" "msg_type=discover_objects&object_type=Service&limit=10"
echo ""

echo "Step 6: Unregister Service"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Unregister order service" '{"msg_type":"unregister_object","payload":{"object_id":"order-service-1","object_type":"Service"}}'
echo ""

echo -e "${GREEN}✅ Service Registry Test Complete${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} Register services with metadata (type, category, capabilities)"
echo -e "  ${GREEN}•${NC} Lookup specific services by ID"
echo -e "  ${GREEN}•${NC} Discover services by type"
echo -e "  ${GREEN}•${NC} Unregister services on shutdown"
echo ""
echo -e "${YELLOW}💡 RegistryFacet intercepts registry operations and uses ObjectRegistry${NC}"
echo ""
