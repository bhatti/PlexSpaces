#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# MiniClaw Python WASM — integration test script
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/miniclaw_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="py-miniclaw"


ask() {
  local actor="$1" payload="$2" timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    -d "$payload"
}

tell() {
  local actor="$1" payload="$2"
  curl -s --max-time 10 -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor" \
    -H "Content-Type: application/json" \
    -d "$payload" >/dev/null
}

check() {
  local desc="$1" response="$2"
  if echo "$response" | grep -qE '"status"\s*:\s*"ok"|"success"\s*:\s*true'; then
    echo -e "  ${GREEN}✓${NC} $desc"
  else
    echo -e "  ${RED}✗${NC} $desc: $response"
    return 1
  fi
}

echo -e "${BOLD}"
echo "================================================================"
echo "  MiniClaw — Mini Agent Framework (Python WASM)"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
echo "  10 actors · agent loop · tool calling · channel queue · FSM"
echo "================================================================"
echo -e "${NC}"

# ── Step 0: Build ────────────────────────────────────────────────────────────
echo "Step 0: Build WASM"
bash "$SCRIPT_DIR/build.sh"
echo ""

# ── Step 1: Node check ───────────────────────────────────────────────────────
echo "Step 1: Check node"
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
  exit 1
fi
echo -e "  ${GREEN}✓${NC} Node running"
echo ""

# ── Step 2: Deploy ───────────────────────────────────────────────────────────
echo "Step 2: Deploy MiniClaw application"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"

TEMP_CONFIG=$(mktemp /tmp/miniclaw-config-XXXXXX.toml)
python3 - <<EOF
import sys, re
with open('$CONFIG_FILE') as f:
    lines = f.readlines()
result = []
for line in lines:
    if line.startswith('seed_nodes = '):
        result.append(f'seed_nodes = ["localhost:$HTTP_PORT"]\n')
    else:
        result.append(line)
with open('$TEMP_CONFIG', 'w') as f:
    f.writelines(result)
EOF

_deployed=0
for _attempt in 1 2 3; do
DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=py-miniclaw" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG")
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
rm -f "$TEMP_CONFIG"

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
  echo -e "  ${GREEN}✓${NC} Deployed"
else
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
sleep 2
echo ""

# ── Step 3: LLM Router ───────────────────────────────────────────────────────
echo "Step 3: LLM Router"
R=$(ask "llm_router" '{"op":"chat_completion","messages":[{"role":"user","content":"Hello!"}],"tools":[]}')
check "chat_completion (end_turn)" "$R"
R=$(ask "llm_router" '{"op":"get_stats"}')
check "get_stats" "$R"
echo ""

# ── Step 4: Tool Registry ────────────────────────────────────────────────────
echo "Step 4: Tool Registry"
R=$(ask "tool_registry" '{"op":"list_tools"}')
check "list_tools" "$R"
R=$(ask "tool_registry" '{"op":"execute_tool","name":"calculator","input":{"expression":"2+2"}}')
check "execute calculator" "$R"
R=$(ask "tool_registry" '{"op":"execute_tool","name":"weather","input":{"location":"London"}}')
check "execute weather" "$R"
echo ""

# ── Step 5: Agent chat ───────────────────────────────────────────────────────
echo "Step 5: Agent chat"
R=$(ask "agent" '{"op":"chat","message":"What is the capital of France?","session_id":"test-sess-1"}' 30)
check "simple chat" "$R"
R=$(ask "agent" '{"op":"chat","message":"Search for the weather in Paris","session_id":"test-sess-1"}' 30)
check "tool-use chat (weather search)" "$R"
R=$(ask "agent" '{"op":"get_stats"}')
check "get_stats" "$R"
echo ""

# ── Step 6: Session Manager ──────────────────────────────────────────────────
echo "Step 6: Session Manager"
R=$(ask "session_manager" '{"op":"create_session","channel":"web","user_id":"alice","agent_id":"agent"}')
check "create_session" "$R"
SESSION_ID=$(echo "$R" | grep -o '"session_id":"[^"]*"' | cut -d'"' -f4 || echo "")
if [ -n "$SESSION_ID" ]; then
  R=$(ask "session_manager" "{\"op\":\"get_session\",\"session_id\":\"$SESSION_ID\"}")
  check "get_session by id" "$R"
fi
R=$(ask "session_manager" '{"op":"list_sessions"}')
check "list_sessions" "$R"
echo ""

# ── Step 7: Memory ───────────────────────────────────────────────────────────
echo "Step 7: Memory"
R=$(ask "memory" '{"op":"store_memory","key":"user_pref","value":"prefers_dark_mode","scope":"global"}')
check "store_memory" "$R"
R=$(ask "memory" '{"op":"recall_memory","key":"user_pref","scope":"global"}')
check "recall_memory" "$R"
R=$(ask "memory" '{"op":"list_memories","scope":"global"}')
check "list_memories" "$R"
echo ""

# ── Step 8: Task Queue (Channel-backed) ──────────────────────────────────────
echo "Step 8: Task Queue (Channel-backed)"
R=$(ask "task_queue" '{"op":"enqueue","task_type":"send_email","payload":{"to":"bob@example.com"}}')
check "enqueue task" "$R"
MSG_ID=$(echo "$R" | grep -o '"msg_id":"[^"]*"' | cut -d'"' -f4 || echo "")
R=$(ask "task_queue" '{"op":"enqueue","task_type":"process_file","payload":{"file":"report.csv"}}')
check "enqueue second task" "$R"
R=$(ask "task_queue" '{"op":"dequeue","limit":1}')
check "dequeue" "$R"
if [ -n "$MSG_ID" ]; then
  R=$(ask "task_queue" "{\"op\":\"ack\",\"msg_id\":\"$MSG_ID\"}")
  check "ack task" "$R"
fi
R=$(ask "task_queue" '{"op":"get_stats"}')
check "get_stats" "$R"
echo ""

# ── Step 9: Agent FSM ────────────────────────────────────────────────────────
echo "Step 9: Agent FSM"
R=$(ask "agent_fsm" '{"op":"get_state"}')
check "get_state (idle)" "$R"
R=$(ask "agent_fsm" '{"op":"transition","to":"processing"}')
check "transition idle → processing" "$R"
R=$(ask "agent_fsm" '{"op":"transition","to":"responding"}')
check "transition processing → responding" "$R"
R=$(ask "agent_fsm" '{"op":"transition","to":"idle"}')
check "transition responding → idle" "$R"
echo ""

# ── Step 10: Orchestrator workflow ───────────────────────────────────────────
echo "Step 10: Orchestrator workflow"
R=$(ask "orchestrator" '{"op":"workflow_run","task":"explain AI agents","task_id":"test-orch-1"}' 60)
check "workflow_run" "$R"
R=$(ask "orchestrator" '{"op":"workflow_query:status"}')
check "workflow_query:status (completed)" "$R"
echo ""

# ── Step 11: Audit events ────────────────────────────────────────────────────
echo "Step 11: Audit events"
tell "audit_event" '{"op":"log_event","event_type":"test_event","detail":"integration test run"}'
R=$(ask "audit_event" '{"op":"get_stats"}')
check "get_stats" "$R"
echo ""

# ── Step 12: Health Monitor ──────────────────────────────────────────────────
echo "Step 12: Health Monitor"
R=$(ask "health_monitor" '{"op":"get_health"}')
check "get_health" "$R"
R=$(ask "health_monitor" '{"op":"get_stats"}')
check "get_stats" "$R"
echo ""

echo -e "${GREEN}${BOLD}All steps passed!${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} GenServer (LLM, ToolRegistry, Agent, SessionManager, Memory, TaskQueue, HealthMonitor)"
echo -e "  ${GREEN}•${NC} Workflow with run/signal/query (Orchestrator)"
echo -e "  ${GREEN}•${NC} GenEvent fire-and-forget (AuditEvent)"
echo -e "  ${GREEN}•${NC} GenFSM state machine (AgentStateFSM)"
echo -e "  ${GREEN}•${NC} Channel as Message Queue (TaskQueue)"
echo -e "  ${GREEN}•${NC} Process Groups for service discovery"
echo -e "  ${GREEN}•${NC} KV for session persistence"
echo -e "  ${GREEN}•${NC} TupleSpace for coordination"
echo -e "  ${GREEN}•${NC} send_after for polling loops"
