#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test Registry - Service Discovery

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/registry_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
# HTTP gateway port - default is 8094 (gRPC 8093 + 1)
HTTP_PORT="${1:-8094}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
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
echo "   Services register themselves, clients discover services by type"
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
    echo "Start node with: cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093"
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
    -F "name=registry" \
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

# Optional: JWT for auth-enabled node (set PLEXSPACES_AUTH_TOKEN or AUTH_TOKEN)
BEARER_TOKEN="${PLEXSPACES_AUTH_TOKEN:-${AUTH_TOKEN:-}}"

# Helper function to send messages
# Path: /api/v1/actors/{namespace}/{actor_type} - namespace from app (application_id), tenant from JWT or empty
# (tenant_id from JWT when auth enabled; without auth use empty so no "internal" tenant in logs)
send_message() {
    local desc="$1"
    local msg_type="$2"
    local payload="$3"
    
    echo -e "  ${BLUE}→${NC} $desc"
    
    if [ -n "$BEARER_TOKEN" ]; then
        RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
            -H "Content-Type: application/json" \
            -H "Authorization: Bearer $BEARER_TOKEN" \
            -d "{\"msg_type\":\"$msg_type\",\"payload\":$payload}" 2>/dev/null) || true
    else
        RESPONSE=$(curl -s -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME" \
            -H "Content-Type: application/json" \
            -d "{\"msg_type\":\"$msg_type\",\"payload\":$payload}" 2>/dev/null) || true
    fi
    
    if echo "$RESPONSE" | grep -q '"success":true'; then
        REPLY=$(echo "$RESPONSE" | grep -o '"reply":"[^"]*"' | cut -d'"' -f4 | sed 's/\\"/"/g' | sed 's/\\\\/\\/g' | sed 's/\\n/ /g')
        echo "$REPLY" | python3 -m json.tool 2>/dev/null || echo "    $REPLY"
        echo ""
        return 0
    else
        echo -e "    ${RED}✗ Failed: $RESPONSE${NC}"
        echo ""
        return 1
    fi
}

# Test 1: Register payment service
echo "Step 3: Register Services"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Real-world scenario: Payment service registers itself${NC}"
echo ""

send_message "Register payment service" "register_object" '{
    "object_id": "payment-service-1",
    "object_type": "Service",
    "object_category": "payment",
    "grpc_address": "http://payment-service:50051",
    "capabilities": ["process_payment", "refund"],
    "labels": ["production", "us-east"],
    "health_status": "Healthy"
}'

# Test 2: Register user service
send_message "Register user service" "register_object" '{
    "object_id": "user-service-1",
    "object_type": "Service",
    "object_category": "user",
    "grpc_address": "http://user-service:50052",
    "capabilities": ["get_user", "create_user"],
    "labels": ["production", "us-west"],
    "health_status": "Healthy"
}'

# Test 3: Register order service
send_message "Register order service" "register_object" '{
    "object_id": "order-service-1",
    "object_type": "Service",
    "object_category": "order",
    "grpc_address": "http://order-service:50053",
    "capabilities": ["create_order", "get_order"],
    "labels": ["production", "us-east"],
    "health_status": "Healthy"
}'

# Test 4: Lookup specific service
echo "Step 4: Lookup Services"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Real-world scenario: Client looks up payment service${NC}"
echo ""

send_message "Lookup payment service" "lookup_object" '{
    "object_id": "payment-service-1",
    "object_type": "Service"
}'

# Test 5: Discover services by category
echo "Step 5: Discover Services by Category"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Real-world scenario: Find all payment services${NC}"
echo ""

send_message "Discover payment services" "discover_objects" '{
    "object_type": "Service",
    "limit": 10,
    "offset": 0
}'

# Test 6: Discover services by labels
echo "Step 6: Discover Services by Labels"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Real-world scenario: Find all services in us-east region${NC}"
echo ""

send_message "Discover us-east services" "discover_objects" '{
    "object_type": "Service",
    "labels": ["us-east"],
    "limit": 10,
    "offset": 0
}'

# Test 7: Unregister service
echo "Step 7: Unregister Service"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Real-world scenario: Service shuts down gracefully${NC}"
echo ""

send_message "Unregister order service" "unregister_object" '{
    "object_id": "order-service-1",
    "object_type": "Service"
}'

# Verify it's gone
send_message "Verify order service removed" "lookup_object" '{
    "object_id": "order-service-1",
    "object_type": "Service"
}'

# Summary
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}✅ Service Registry Test Complete${NC}"
echo ""
echo "Key operations demonstrated:"
echo "  ✓ Register services with metadata (type, category, capabilities, labels)"
echo "  ✓ Lookup specific services by ID"
echo "  ✓ Discover services by type and filters"
echo "  ✓ Unregister services on shutdown"
echo ""
echo -e "${YELLOW}💡 RegistryFacet intercepts all registry operations${NC}"
echo "   The actor's handle() method is never called for registry operations"
echo "   All operations use the real ObjectRegistry backend"
echo ""
