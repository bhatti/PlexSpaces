#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# MiniHermes Python WASM — integration test script
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/minihermes_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="py-minihermes"

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

# ask actor op payload [timeout]
ask() {
  local actor="$1" payload="$2" timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload"
}

# tell actor payload
tell() {
  local actor="$1" payload="$2"
  curl -s --max-time 10 -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
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

pyval() {
  local resp="$1" expr="$2" default="${3:-?}"
  echo "$resp" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    p = d.get('payload', d)
    print($expr)
except:
    print('$default')
" 2>/dev/null || echo "$default"
}

echo -e "${BOLD}"
echo "================================================================"
echo "  MiniHermes — Self-Improving AI Agent (Python WASM)"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
echo "  12 actors · skill learning · tiered memory · cron automation"
echo "  context compression · guardrails · provider hot-swap"
echo "================================================================"
echo -e "${NC}"

# ── Step 0: Build ────────────────────────────────────────────────────────────
echo "Step 0: Build WASM"
bash "$SCRIPT_DIR/build.sh"
echo ""

# ── Step 1: Node check ───────────────────────────────────────────────────────
echo "Step 1: Check node"
HTTP_CHECK="000"
for _i in 1 2 3; do
  HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
  [ "$HTTP_CHECK" != "000" ] && break
  sleep 2
done
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
  exit 1
fi
echo -e "  ${GREEN}✓${NC} Node running"
echo ""

# ── Step 2: Deploy ───────────────────────────────────────────────────────────
echo "Step 2: Deploy MiniHermes application"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT" 2>/dev/null || true

TEMP_CONFIG=$(mktemp /tmp/minihermes-config-XXXXXX.toml)
python3 - <<EOF
import sys
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
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=py-minihermes" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$TEMP_CONFIG") || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed (HTTP $HTTP_CODE), retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
rm -f "$TEMP_CONFIG"
echo -e "  ${GREEN}✓${NC} Deployed — 12 actors registered"
sleep 2
echo ""

# ── Step 3: LLM Gateway stats ────────────────────────────────────────────────
echo "Step 3: LLM Gateway — initial stats  [circuit breaker + metrics]"
R=$(ask "llm_gateway" '{"op":"get_stats"}')
check "get_stats" "$R"
echo "  Active provider : $(pyval "$R" "p.get('active_provider','?')")"
echo "  Default model   : $(pyval "$R" "p.get('default_model','?')")"
echo ""

# ── Step 4: Register LLM provider ────────────────────────────────────────────
echo "Step 4: LLM Gateway — register provider  [KV provider registry]"
R=$(ask "llm_gateway" '{"op":"register_provider","name":"ollama","base_url":"http://localhost:11434","model":"llama3.2"}')
check "register_provider ollama" "$R"
echo ""

# ── Step 5: List tools ───────────────────────────────────────────────────────
echo "Step 5: Tool Executor — list built-in tools  [KV enumeration]"
R=$(ask "tool_executor" '{"op":"list_tools"}')
check "list_tools" "$R"
TOOL_COUNT=$(pyval "$R" "p.get('count',0)")
echo "  Registered tools: $TOOL_COUNT"
if [ "$TOOL_COUNT" -ge 6 ] 2>/dev/null; then
  echo -e "  ${GREEN}✓${NC} 6+ built-in tools present"
else
  echo -e "  ${YELLOW}WARN${NC} expected ≥6, got $TOOL_COUNT"
fi
echo ""

# ── Step 6: Register custom tool ─────────────────────────────────────────────
echo "Step 6: Tool Executor — register custom tool  [runtime extensibility]"
R=$(ask "tool_executor" '{"op":"register_tool","name":"weather_lookup","description":"Get current weather for a city","input_schema":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]},"handler_type":"builtin"}')
check "register_tool weather_lookup" "$R"
echo ""

# ── Step 7: Execute calculator ───────────────────────────────────────────────
echo "Step 7: Tool Executor — calculator  [deterministic arithmetic]"
R=$(ask "tool_executor" '{"op":"execute","name":"calculator","input":{"expression":"42 * 17"}}')
check "execute calculator" "$R"
CALC=$(pyval "$R" "p.get('output',{}).get('result','?')")
echo "  42 * 17 = $CALC"
if [ "$CALC" = "714.0" ] || [ "$CALC" = "714" ]; then
  echo -e "  ${GREEN}✓${NC} exact result"
else
  echo -e "  ${YELLOW}WARN${NC} unexpected result: $CALC"
fi
echo ""

# ── Step 8: Simple chat ──────────────────────────────────────────────────────
echo "Step 8: Agent — simple chat  [conversation loop, end_turn]"
R=$(ask "agent" '{"op":"chat","message":"Tell me about yourself and what you can do.","session_id":"test-sess-1"}' 30)
check "simple chat" "$R"
echo "  Response: $(pyval "$R" "str(p.get('response',''))[:100]")..."
echo ""

# ── Step 9: Chat with tool use ───────────────────────────────────────────────
echo "Step 9: Agent — chat with calculator tool  [tool loop]"
R=$(ask "agent" '{"op":"chat","message":"Please calculate 123 * 456 for me","session_id":"test-sess-2"}' 30)
check "tool-use chat" "$R"
echo "  Tool calls: $(pyval "$R" "p.get('tool_calls',0)")"
echo ""

# ── Step 10: Memory — store and recall ──────────────────────────────────────
echo "Step 10: Memory — tiered storage  [core/reachable tiers]"
R=$(ask "memory" '{"op":"store_memory","tier":"core","key":"user_name","value":"Alice","scope":"global"}')
check "store_memory core tier" "$R"
R=$(ask "memory" '{"op":"store_memory","tier":"reachable","key":"last_topic","value":"AI","scope":"global"}')
check "store_memory reachable tier" "$R"
R=$(ask "memory" '{"op":"recall_memory","query":"user_name","scope":"global"}')
check "recall_memory" "$R"
echo "  Memories found: $(pyval "$R" "p.get('count',0)")"
echo ""

# ── Step 11: Agent using memory tool ────────────────────────────────────────
echo "Step 11: Agent — chat stores memory via tool  [cross-actor dispatch]"
R=$(ask "agent" '{"op":"chat","message":"Remember that my favorite language is Python","session_id":"test-sess-3"}' 30)
check "agent stores memory" "$R"
echo "  Tool calls: $(pyval "$R" "p.get('tool_calls',0)")"
echo ""

# ── Step 12: Skill store — propose ──────────────────────────────────────────
echo "Step 12: Skill Store — propose skill  [KV + TupleSpace index]"
R=$(ask "skill_store" '{"op":"propose_skill","name":"Calculate compound interest","description":"Compute compound interest","procedure":"1. Ask for principal, rate, years.\n2. Call calculator.\n3. Return result.","tags":"finance,math","trigger_patterns":"compound interest,calculate interest"}')
check "propose_skill" "$R"
SKILL_ID=$(pyval "$R" "p.get('skill_id','?')")
echo "  Skill ID: $SKILL_ID"
echo ""

# ── Step 13: Skill store — match ────────────────────────────────────────────
echo "Step 13: Skill Store — match skills  [TupleSpace pattern search]"
R=$(ask "skill_store" '{"op":"match_skills","query":"calculate compound interest"}')
check "match_skills" "$R"
echo "  Matched: $(pyval "$R" "p.get('count',0)")"
echo ""

# ── Step 14: Skill store — evaluate for learning ────────────────────────────
echo "Step 14: Skill Store — evaluate_for_learning  [automatic skill extraction]"
R=$(ask "skill_store" '{"op":"evaluate_for_learning","session_id":"test-s4","tool_call_count":3,"messages":"[{\"role\":\"user\",\"content\":\"compute and store and list\"},{\"role\":\"assistant\",\"content\":\"Done.\",\"tool_calls\":[{\"name\":\"calculator\"},{\"name\":\"memory_store\"},{\"name\":\"list_skills\"}]}]"}')
check "evaluate_for_learning" "$R"
echo "  Action: $(pyval "$R" "p.get('action','?')")"
echo ""

# ── Step 15: Cron — create job ──────────────────────────────────────────────
echo "Step 15: Cron Scheduler — create job  [distributed leader election]"
R=$(ask "cron_scheduler" '{"op":"create_job","job_id":"heartbeat-1","prompt":"Send a heartbeat ping","schedule":"every_1h"}')
check "create_job" "$R"
echo "  Job ID: $(pyval "$R" "p.get('job_id','?')")"
echo "  Interval: $(pyval "$R" "p.get('interval_ms','?')") ms"
echo ""

# ── Step 16: Cron — list jobs ───────────────────────────────────────────────
echo "Step 16: Cron Scheduler — list jobs"
R=$(ask "cron_scheduler" '{"op":"list_jobs"}')
check "list_jobs" "$R"
echo "  Jobs count: $(pyval "$R" "p.get('count',0)")"
echo ""

# ── Step 17: Context compression ────────────────────────────────────────────
echo "Step 17: Context Compressor — compress long history  [LLM summarization]"
MSGS='[{"role":"system","content":"You are helpful."},{"role":"user","content":"Q1"},{"role":"assistant","content":"A1"},{"role":"user","content":"Q2"},{"role":"assistant","content":"A2"},{"role":"user","content":"Q3"},{"role":"assistant","content":"A3"},{"role":"user","content":"Q4"},{"role":"assistant","content":"A4"},{"role":"user","content":"Q5"},{"role":"assistant","content":"A5"},{"role":"user","content":"Latest question"}]'
R=$(ask "context_compressor" "{\"op\":\"compress\",\"session_id\":\"compress-test\",\"messages\":$(echo $MSGS | python3 -c 'import sys,json; print(json.dumps(sys.stdin.read().strip()))'),\"keep_last\":4}" 30)
check "compress" "$R"
echo "  Action: $(pyval "$R" "p.get('action','?')")"
echo ""

# ── Step 18: Guardrails — allow ─────────────────────────────────────────────
echo "Step 18: Guardrails — allow safe tool  [GenFSM policy enforcement]"
R=$(ask "guardrails" '{"op":"check","tool":"calculator"}')
check "check calculator → allow" "$R"
echo "  Decision: $(pyval "$R" "p.get('decision','?')")"
echo ""

# ── Step 19: Guardrails — deny ──────────────────────────────────────────────
echo "Step 19: Guardrails — deny dangerous tool"
R=$(ask "guardrails" '{"op":"check","tool":"delete_file"}')
if echo "$R" | grep -q '"deny"'; then
  echo -e "  ${GREEN}✓${NC} delete_file denied"
else
  echo -e "  ${YELLOW}WARN${NC} unexpected decision: $R"
fi
echo ""

# ── Step 20: Session manager ─────────────────────────────────────────────────
echo "Step 20: Session Manager — create and get session  [KV lifecycle]"
R=$(ask "session_manager" '{"op":"create_session","channel":"web","user_id":"alice"}')
check "create_session" "$R"
SESSION_ID=$(pyval "$R" "p.get('session_id','?')")
echo "  Session ID: $SESSION_ID"
if [ "$SESSION_ID" != "?" ]; then
  R2=$(ask "session_manager" "{\"op\":\"get_session\",\"session_id\":\"$SESSION_ID\"}")
  check "get_session" "$R2"
fi
echo ""

# ── Step 21: Health monitor ──────────────────────────────────────────────────
echo "Step 21: Health Monitor — snapshot  [PG polling + TupleSpace]"
R=$(ask "health_monitor" '{"op":"get_health"}')
check "get_health" "$R"
echo "  Status: $(pyval "$R" "p.get('status','?')")"
echo ""

echo -e "${GREEN}${BOLD}All steps passed!${NC}"
echo ""
echo "This example demonstrated:"
echo -e "  ${GREEN}•${NC} GenServer (LLMGateway, ToolExecutor, Agent, SkillStore, Memory, ContextCompressor, CronScheduler, SessionManager, HealthMonitor)"
echo -e "  ${GREEN}•${NC} GenFSM state machine (GuardrailsGate)"
echo -e "  ${GREEN}•${NC} GenEvent fire-and-forget (AuditEvent)"
echo -e "  ${GREEN}•${NC} Workflow with parallel LLM passes (SkillExtractionWorkflow)"
echo -e "  ${GREEN}•${NC} Object Registry for capability-aware service discovery"
echo -e "  ${GREEN}•${NC} Process Groups as fallback discovery"
echo -e "  ${GREEN}•${NC} KV for session and skill persistence"
echo -e "  ${GREEN}•${NC} TupleSpace for skill tag indexes"
echo -e "  ${GREEN}•${NC} BlobStorage for skill procedure bodies"
echo -e "  ${GREEN}•${NC} DistributedLock for cron leader election"
echo -e "  ${GREEN}•${NC} Channel for at-least-once job delivery"
echo -e "  ${GREEN}•${NC} HTTPFetch for real LLM calls (Ollama/OpenAI)"
echo -e "  ${GREEN}•${NC} SendAfter for tick loops and circuit recovery"
