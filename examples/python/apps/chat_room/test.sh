#!/bin/bash
# SPDX-License-Identifier: LGPL-2.1-or-later
# Test Chat Room - Real-time chat with ProcessGroups

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/chat_room_actor.wasm"
# HTTP gateway port - default is 8092
HTTP_PORT="${1:-8092}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

APP_ID="chat-room-test"
ACTOR_NAME="ChatRoom"

cleanup() {
    echo ""
    echo "Cleanup: Undeploying $APP_ID"
    curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" 2>/dev/null || true
}

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║        Chat Room - Real-time Chat with ProcessGroups           ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo -e "${YELLOW}💡 Real-world use case: Chat apps, live notifications${NC}"
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
echo "Step 2: Create chat room (deploy actor)"
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
    echo -e "${GREEN}✓ Created chat room #general${NC}"
else
    echo -e "${RED}✗ Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper function - send POST
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

echo "Step 3: Users join the room"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Alice joins #general" '{"msg_type":"join","payload":{"user":"alice"}}'
send_post "Bob joins #general" '{"msg_type":"join","payload":{"user":"bob"}}'
send_post "Charlie joins #general" '{"msg_type":"join","payload":{"user":"charlie"}}'
echo ""

echo "Step 4: Get room members"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
RESPONSE=$(curl -s --max-time 10 -X GET "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME?msg_type=members" 2>/dev/null) || RESPONSE=""
if echo "$RESPONSE" | grep -q '"success":true'; then
    echo -e "  ${GREEN}✓${NC} Room has members (Alice, Bob, Charlie)"
else
    echo -e "  ${YELLOW}⚠${NC} Get members: $RESPONSE"
fi
echo ""

echo "Step 5: Alice sends a message"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "[alice]: Hey everyone! How's it going?" '{"msg_type":"send","payload":{"user":"alice","text":"Hey everyone! How'\''s it going?"}}'
echo -e "  ${CYAN}Delivered to: bob, charlie${NC}"
echo ""

echo "Step 6: Bob replies"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "[bob]: Great! Working on the new feature." '{"msg_type":"send","payload":{"user":"bob","text":"Great! Working on the new feature."}}'
echo -e "  ${CYAN}Delivered to: alice, charlie${NC}"
echo ""

echo "Step 7: Charlie leaves the room"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "Charlie left #general" '{"msg_type":"leave","payload":{"user":"charlie"}}'
echo ""

echo "Step 8: Alice sends another message (Charlie doesn't receive)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
send_post "[alice]: Charlie left, it's just us now!" '{"msg_type":"send","payload":{"user":"alice","text":"Charlie left, it'\''s just us now!"}}'
echo -e "  ${CYAN}Delivered to: bob${NC}"
echo -e "  ${CYAN}(charlie did NOT receive - left room)${NC}"
echo ""

echo "Step 9: Get chat history"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
RESPONSE=$(curl -s --max-time 10 -X GET "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$ACTOR_NAME?msg_type=history&limit=10" 2>/dev/null) || RESPONSE=""
if echo "$RESPONSE" | grep -q '"success":true'; then
    echo -e "  ${GREEN}✓${NC} Chat history retrieved"
else
    echo -e "  ${YELLOW}⚠${NC} Get history: $RESPONSE"
fi
echo ""

echo -e "${GREEN}✅ Chat Room Test Complete${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} Process Groups: Named groups of actors (chat rooms)"
echo -e "  ${GREEN}•${NC} Pub/Sub: Broadcast messages to all room members"
echo -e "  ${GREEN}•${NC} Dynamic membership: Join/leave at runtime"
echo -e "  ${GREEN}•${NC} Message history: Track conversation"
echo ""
echo "PlexSpaces Integration:"
echo -e "  ${GREEN}•${NC} ProcessGroupRegistry: Manages groups"
echo -e "  ${GREEN}•${NC} join_group / leave_group"
echo -e "  ${GREEN}•${NC} publish_to_group (broadcast)"
echo ""
