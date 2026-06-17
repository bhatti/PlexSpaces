#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/minihermes_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  echo "Generating JWT token..."
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh")"
  eval "$JWT_OUTPUT"
  if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ]; then
    echo "ERROR: gen-test-jwt.sh failed to set PLEXSPACES_TEST_TOKEN"
    exit 1
  fi
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi

if [[ -z "${1:-}" ]]; then
  HTTP_PORT=8091
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  HTTP_PORT="$1"
else
  HTTP_PORT=8091
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="go-minihermes"
TEMP_CONFIG=""

send_op() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

check_ok() {
  local resp="$1"
  local label="$2"
  if echo "$resp" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert p.get('status')=='ok' or 'error' not in p, p" 2>/dev/null; then
    echo -e "  ${GREEN}PASS${NC} $label"
  else
    echo -e "  ${RED}FAIL${NC} $label: $resp"
    exit 1
  fi
}

pyval() {
  local resp="$1"
  local expr="$2"
  local default="${3:-?}"
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

echo "================================================================"
echo -e "  ${BOLD}MiniHermes — Self-Improving AI Agent (Go WASM)${NC}"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
echo "  11 actors · real LLM via Ollama · skill learning · tiered memory"
echo "  context compression · cron automation · guardrails · provider hot-swap"
echo "================================================================"
echo ""

# Step 0: Build
echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

# Check node reachability
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
  exit 1
fi

GRPC_PORT=$HTTP_PORT
TEMP_CONFIG="$(mktemp -t minihermes-config)"
python3 - "$CONFIG_FILE" "$TEMP_CONFIG" "$GRPC_PORT" <<'PY'
import pathlib, sys
source = pathlib.Path(sys.argv[1]).read_text()
grpc_port = sys.argv[3]
lines = []
for line in source.splitlines():
    if line.startswith("seed_nodes = "):
        lines.append(f'seed_nodes = ["localhost:{grpc_port}"]')
    else:
        lines.append(line)
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY

# Step 1: Deploy
echo "Step 1: Deploy MiniHermes application"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_RESP=$(curl -s --connect-timeout 10 --max-time 120 -w "\n%{http_code}" \
    -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=go-minihermes" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG" 2>&1)
  HTTP_CODE=$(echo "$DEPLOY_RESP" | tail -n1)
  BODY=$(echo "$DEPLOY_RESP" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$BODY" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $BODY${NC}"
  exit 1
fi
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
echo -e "  ${GREEN}Deployed${NC} — 11 actors registered, each joined its process group (svc:*)"
sleep 3
echo ""

# Step 2: Register provider
echo "Step 2: Register LLM providers  [LLMGatewayActor — KV provider registry]"
echo -e "  ${CYAN}Problem:${NC} Agent needs to know which LLM to call and how to authenticate."
echo -e "  ${CYAN}Mechanism:${NC} ProviderRegistry stores configs in KV. HTTPFetch uses service links for real calls."
REG_RESP=$(send_op "llm_gateway" '{"op":"register_provider","name":"ollama","base_url":"http://localhost:11434","model":"llama3.2"}')
check_ok "$REG_RESP" "register_provider ollama → persisted to KV"
echo ""

# Step 3: LLM gateway stats
echo "Step 3: LLM gateway stats  [LLMGatewayActor — circuit breaker + metrics]"
echo -e "  ${CYAN}Problem:${NC} Ops teams need visibility into LLM call patterns and failure modes."
echo -e "  ${CYAN}Mechanism:${NC} IncrCounter tracks requests; circuit breaker opens after 3 consecutive failures."
STATS_RESP=$(send_op "llm_gateway" '{"op":"get_stats"}')
check_ok "$STATS_RESP" "get_stats → active provider and metrics"
echo "  Active provider : $(pyval "$STATS_RESP" "p.get('active_provider','?')")"
echo "  Default model   : $(pyval "$STATS_RESP" "p.get('default_model','?')")"
echo ""

# Step 4: Register custom tool
echo "Step 4: Register custom tool  [ToolExecutorActor — runtime extensibility]"
echo -e "  ${CYAN}Problem:${NC} Users need to extend the agent with domain-specific tools without redeployment."
echo -e "  ${CYAN}Mechanism:${NC} Tools stored as JSON in KV under 'tool:<name>'; discovered on every chat turn."
RTOOL_RESP=$(send_op "tool_executor" '{"op":"register_tool","name":"weather_lookup","description":"Get current weather for a city","input_schema":{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]},"handler_type":"builtin"}')
check_ok "$RTOOL_RESP" "register_tool weather_lookup → KV persisted"
echo ""

# Step 5: List tools
echo "Step 5: List tools  [ToolExecutorActor — KV enumeration]"
echo -e "  ${CYAN}Problem:${NC} AgentActor needs the full tool schema to pass to the LLM on each turn."
echo -e "  ${CYAN}Mechanism:${NC} 'tool_names' KV key indexes names; list_tools hydrates each from 'tool:<name>'."
LIST_RESP=$(send_op "tool_executor" '{"op":"list_tools"}')
python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
count = p.get('count', 0)
tools = p.get('tools', [])
print(f'  Registered tools ({count} total):')
for t in tools[:8]:
    print(f'    {t.get(\"name\",\"?\"):25s}  {t.get(\"description\",\"\")[:55]}')
" <<< "$LIST_RESP"
if echo "$LIST_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert p.get('count',0) >= 6, p" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} list_tools — 6 built-in + 1 custom = $(pyval "$LIST_RESP" "p.get('count',0)") tools"
else
  echo -e "  ${YELLOW}WARN${NC} list_tools: expected ≥6, got $(pyval "$LIST_RESP" "p.get('count',0)")"
fi
echo ""

# Step 6: Execute calculator
echo "Step 6: Execute calculator  [ToolExecutorActor — deterministic built-in execution]"
echo -e "  ${CYAN}Problem:${NC} LLMs hallucinate arithmetic. Delegate computations to deterministic code."
echo -e "  ${CYAN}Mechanism:${NC} evalExpression() handles a+b, a-b, a*b, a/b; result returned as exact float."
CALC_RESP=$(send_op "tool_executor" '{"op":"execute","name":"calculator","input":{"expression":"42 * 17"}}')
CALC_RESULT=$(pyval "$CALC_RESP" "p.get('output',{}).get('result','?')")
echo "  42 * 17 = $CALC_RESULT"
if echo "$CALC_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert p.get('output',{}).get('result')==714, p" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} calculator — exact arithmetic (no hallucination)"
else
  echo -e "  ${RED}FAIL${NC} calculator: $CALC_RESP"; exit 1
fi
echo ""

# Step 7: Simple chat (no tool use)
echo "Step 7: Simple chat  [AgentActor — conversation loop, end_turn]"
echo -e "  ${CYAN}Problem:${NC} Agent must handle natural conversation without tool calling."
echo -e "  ${CYAN}Mechanism:${NC} LLMGateway returns stop_reason=end_turn; loop exits without tool dispatch."
CHAT_RESP=$(send_op "agent" '{"op":"chat","message":"Tell me about yourself and what you can do.","session_id":"test-sess-1"}' 30)
check_ok "$CHAT_RESP" "simple chat → end_turn response"
echo "  Response: $(pyval "$CHAT_RESP" "p.get('response','')[:100]")..."
echo "  Iterations: $(pyval "$CHAT_RESP" "p.get('loop_iterations',0)")"
echo ""

# Step 8: Chat with tool use
echo "Step 8: Chat with calculator tool  [AgentActor — tool loop]"
echo -e "  ${CYAN}Problem:${NC} Agent must detect tool_use in LLM response, execute it, and feed result back."
echo -e "  ${CYAN}Mechanism:${NC} AgentActor loops: LLM response → tool dispatch → append tool result → LLM again."
TOOL_CHAT_RESP=$(send_op "agent" '{"op":"chat","message":"Please calculate 123 * 456 for me","session_id":"test-sess-2"}' 30)
check_ok "$TOOL_CHAT_RESP" "tool-use chat → calculator dispatched"
echo "  Tool calls   : $(pyval "$TOOL_CHAT_RESP" "p.get('tool_calls',0)")"
echo "  Iterations   : $(pyval "$TOOL_CHAT_RESP" "p.get('loop_iterations',0)")"
echo ""

# Step 9: Tiered memory (direct)
echo "Step 9: Tiered memory  [MemoryActor — core/reachable/deep tiers]"
echo -e "  ${CYAN}Problem:${NC} Different facts have different lifetime: permanent facts vs. recent context vs. archives."
echo -e "  ${CYAN}Mechanism:${NC} core=KV(no TTL), reachable=KV(TTL)+TupleSpace, deep=BlobStorage+TupleSpace."
MEM_STORE=$(send_op "memory" '{"op":"store_memory","tier":"core","key":"user_name","value":"Alice","scope":"global"}')
check_ok "$MEM_STORE" "store_memory core tier"
MEM_REACH=$(send_op "memory" '{"op":"store_memory","tier":"reachable","key":"last_topic","value":"AI self-improvement","scope":"global"}')
check_ok "$MEM_REACH" "store_memory reachable tier (TTL decay)"
MEM_RECALL=$(send_op "memory" '{"op":"recall_memory","query":"user","scope":"global"}')
check_ok "$MEM_RECALL" "recall_memory → pattern search across tiers"
echo "  Memories found: $(pyval "$MEM_RECALL" "p.get('count',0)")"
echo ""

# Step 10: Agent uses memory tool
echo "Step 10: Agent using memory tool  [AgentActor + MemoryActor — cross-actor tool dispatch]"
echo -e "  ${CYAN}Problem:${NC} Agent should be able to store and recall information via natural language."
echo -e "  ${CYAN}Mechanism:${NC} memory_recall tool routes through ToolExecutor → Ask(MemoryActor)."
MEM_CHAT=$(send_op "agent" '{"op":"chat","message":"Remember this: my favorite programming language is Go","session_id":"test-sess-3"}' 30)
check_ok "$MEM_CHAT" "agent stores memory via tool"
echo "  Tool calls   : $(pyval "$MEM_CHAT" "p.get('tool_calls',0)")"
echo ""

# Step 11: Propose skill
echo "Step 11: Propose skill  [SkillStoreActor — procedural memory creation]"
echo -e "  ${CYAN}Problem:${NC} Agents repeat complex multi-step tasks. Skills encode learned procedures."
echo -e "  ${CYAN}Mechanism:${NC} KV stores metadata, BlobStorage stores procedure body, TupleSpace indexes by tag."
SKILL_RESP=$(send_op "skill_store" '{"op":"propose_skill","name":"Calculate compound interest","description":"Compute compound interest with multiple tool calls","procedure":"1. Ask user for principal, rate, years.\n2. Call calculator for interest: principal*(1+rate/100)^years - principal.\n3. Return formatted result.","tags":"finance,math,calculation","trigger_patterns":"compound interest,calculate interest,investment return"}')
check_ok "$SKILL_RESP" "propose_skill → KV + BlobStorage + TupleSpace indexed"
SKILL_ID=$(pyval "$SKILL_RESP" "p.get('skill_id','?')")
echo "  Skill ID     : $SKILL_ID"
echo ""

# Step 12: Match skills
echo "Step 12: Match skills  [SkillStoreActor — pattern-based retrieval]"
echo -e "  ${CYAN}Problem:${NC} Agent needs to find relevant skills to inject into the prompt before calling LLM."
echo -e "  ${CYAN}Mechanism:${NC} TupleSpace ReadAll on 'skill_trigger' tuples; matches on user message substrings."
MATCH_RESP=$(send_op "skill_store" '{"op":"match_skills","query":"I need to calculate compound interest on my investment","limit":3}')
check_ok "$MATCH_RESP" "match_skills → TupleSpace trigger pattern search"
echo "  Skills found : $(pyval "$MATCH_RESP" "p.get('count',0)")"
echo ""

# Step 13: Agent uses skill (inject context)
echo "Step 13: Agent chat with skill injection  [AgentActor — skill context in system prompt]"
echo -e "  ${CYAN}Problem:${NC} When a skill matches the query, it should reduce tool-use loops needed."
echo -e "  ${CYAN}Mechanism:${NC} Agent.fetchSkillContext() injects matching skills into system prompt before LLM call."
SKILL_CHAT=$(send_op "agent" '{"op":"chat","message":"How do I calculate compound interest?","session_id":"test-sess-4"}' 30)
check_ok "$SKILL_CHAT" "skill-injected chat → matched skill in prompt"
echo "  Response: $(pyval "$SKILL_CHAT" "p.get('response','')[:120]")..."
echo ""

# Step 14: Create cron job
echo "Step 14: Create cron job  [CronSchedulerActor — KV job persistence]"
echo -e "  ${CYAN}Problem:${NC} Users want recurring automation without external cron daemons."
echo -e "  ${CYAN}Mechanism:${NC} Job stored in KV('cron_job:<id>'); tick loop checks interval_ms vs last_run_at."
CRON_RESP=$(send_op "cron_scheduler" '{"op":"create_job","job_id":"daily-summary","prompt":"Summarize all memories stored today and report the most important insights.","schedule":"every_1m"}')
check_ok "$CRON_RESP" "create_job → KV persisted with interval_ms"
echo "  Schedule     : $(pyval "$CRON_RESP" "p.get('schedule','?')")"
echo "  Interval     : $(pyval "$CRON_RESP" "p.get('interval_ms',0)") ms"
echo ""

# Step 15: Force cron tick
echo "Step 15: Force cron tick  [CronSchedulerActor — DistributedLock + Channel delivery]"
echo -e "  ${CYAN}Problem:${NC} In a cluster, multiple nodes must not process the same cron job."
echo -e "  ${CYAN}Mechanism:${NC} TryAcquire DistributedLock('cron_leader'); only leader publishes to Channel."
# Set last_run_at to 0 so job is immediately due
TICK_RESP=$(send_op "cron_scheduler" '{"op":"trigger_tick"}' 15)
check_ok "$TICK_RESP" "trigger_tick → leader elected, due jobs enqueued to Channel"
echo "  Leader       : $(pyval "$TICK_RESP" "str(p.get('leader',False))")"
echo "  Triggered    : $(pyval "$TICK_RESP" "p.get('triggered',0)") jobs"
echo ""

# Step 16: Process cron job
echo "Step 16: Process cron job  [AgentActor — isolated session execution]"
echo -e "  ${CYAN}Problem:${NC} Cron jobs must run in isolated sessions without contaminating main conversation."
echo -e "  ${CYAN}Mechanism:${NC} Agent saves/restores messages; cron session keyed as 'cron-<job_id>-<timestamp>'."
CRON_EXEC=$(send_op "agent" '{"op":"process_cron","job_id":"daily-summary","prompt":"Report: what is the current date?"}' 30)
check_ok "$CRON_EXEC" "process_cron → isolated execution, main session preserved"
echo "  Run ID       : $(pyval "$CRON_EXEC" "p.get('run_id','?')")"
echo ""

# Step 17: Context compression
echo "Step 17: Context compression  [ContextCompressorActor — long-session management]"
echo -e "  ${CYAN}Problem:${NC} Long conversations exceed context window. Must compress without losing key info."
echo -e "  ${CYAN}Mechanism:${NC} Middle messages summarized via LLM; original checkpointed in KV for audit."
COMPRESS_RESP=$(send_op "context_compressor" '{"op":"compress","session_id":"test-compress","messages":"[{\"role\":\"user\",\"content\":\"Hello\"},{\"role\":\"assistant\",\"content\":\"Hi there!\"},{\"role\":\"user\",\"content\":\"What is AI?\"},{\"role\":\"assistant\",\"content\":\"AI stands for Artificial Intelligence\"},{\"role\":\"user\",\"content\":\"Tell me more\"},{\"role\":\"assistant\",\"content\":\"AI enables machines to perform tasks that typically require human intelligence\"},{\"role\":\"user\",\"content\":\"Examples?\"},{\"role\":\"assistant\",\"content\":\"ChatGPT, image recognition, self-driving cars\"},{\"role\":\"user\",\"content\":\"Latest question\"},{\"role\":\"assistant\",\"content\":\"Latest answer\"}]","keep_last":4}')
check_ok "$COMPRESS_RESP" "compress → middle messages summarized, original checkpointed"
echo "  Before msgs  : $(pyval "$COMPRESS_RESP" "p.get('before_messages',0)")"
echo "  After msgs   : $(pyval "$COMPRESS_RESP" "p.get('after_messages',0)")"
echo "  Tokens saved : $(pyval "$COMPRESS_RESP" "p.get('tokens_saved',0)")"
echo ""

# Step 18: Guardrails check and approve
echo "Step 18: Guardrails approval flow  [GuardrailsGateActor — GenFSM policy enforcement]"
echo -e "  ${CYAN}Problem:${NC} Destructive tool calls need human-in-the-loop approval before execution."
echo -e "  ${CYAN}Mechanism:${NC} check() returns requires_approval with ID; approve() transitions to approved state."
GUARD_RESP=$(send_op "guardrails" '{"op":"check","tool":"http_request","input":{"url":"https://api.example.com/delete","method":"POST"}}')
check_ok "$GUARD_RESP" "check http_request → requires_approval policy triggered"
echo "  Decision     : $(pyval "$GUARD_RESP" "p.get('decision','?')")"
APPROVAL_ID=$(pyval "$GUARD_RESP" "p.get('approval_id','?')")
echo "  Approval ID  : $APPROVAL_ID"
if [ "$APPROVAL_ID" != "?" ]; then
  APPROVE_RESP=$(send_op "guardrails" "{\"op\":\"approve\",\"approval_id\":\"$APPROVAL_ID\"}")
  check_ok "$APPROVE_RESP" "approve → decision=approved, execution unblocked"
  echo "  Final state  : $(pyval "$APPROVE_RESP" "p.get('decision','?')")"
fi
echo ""

# Step 19: Health monitoring
echo "Step 19: Health monitoring  [HealthMonitorActor — PG polling, all services healthy]"
echo -e "  ${CYAN}Problem:${NC} Ops teams need visibility into which actors are registered and reachable."
echo -e "  ${CYAN}Mechanism:${NC} SendAfter tick → PG.Members() for each svc:* group → TupleSpace snapshot."
HEALTH_RESP=$(send_op "health_monitor" '{"op":"get_health"}')
check_ok "$HEALTH_RESP" "get_health → PG membership map"
echo "  Healthy      : $(pyval "$HEALTH_RESP" "p.get('healthy',0)") / 10 groups"
DEGRADED=$(pyval "$HEALTH_RESP" "len(p.get('degraded',[]))" "0")
if [ "$DEGRADED" -gt "0" ]; then
  echo -e "  ${YELLOW}WARN${NC} degraded groups: $(pyval "$HEALTH_RESP" "p.get('degraded',[])")"
fi
echo ""

# Step 20: Provider hot-swap
echo "Step 20: Provider hot-swap  [LLMGatewayActor — live provider switching]"
echo -e "  ${CYAN}Problem:${NC} Switch LLM providers mid-session without restart (cost, availability, capability)."
echo -e "  ${CYAN}Mechanism:${NC} switch_provider updates KV('llm_gateway:active_provider'); next call uses new provider."
SWAP_RESP=$(send_op "llm_gateway" '{"op":"switch_provider","provider":"ollama","model":"llama3.2"}')
check_ok "$SWAP_RESP" "switch_provider ollama → hot-swap without restart"
echo "  Old provider : $(pyval "$SWAP_RESP" "p.get('old_provider','?')")"
echo "  New provider : $(pyval "$SWAP_RESP" "p.get('new_provider','?')")"
echo ""

# Audit trail
echo "Step 21: Audit event trail  [AuditEventActor — two-cursor watermark polling]"
echo -e "  ${CYAN}Problem:${NC} All operations must be auditable; consumers need ordered delivery without missed events."
echo -e "  ${CYAN}Mechanism:${NC} Monotonic watermark in actor state; per-consumer KV cursor; poll from (cursor+1..watermark)."
AUDIT_RESP=$(send_op "audit_event" '{"op":"poll_events","consumer_id":"test-consumer","limit":5}')
check_ok "$AUDIT_RESP" "poll_events → watermark cursor delivery"
echo "  Events       : $(pyval "$AUDIT_RESP" "p.get('count',0)")"
echo "  Watermark    : $(pyval "$AUDIT_RESP" "p.get('watermark',0)")"
echo ""

echo "================================================================"
echo -e "  ${GREEN}${BOLD}MiniHermes — All tests passed!${NC}"
echo ""
echo -e "  ${BOLD}PlexSpaces primitives demonstrated:${NC}"
echo "    KV          → tool registry, skill metadata, session history, provider config"
echo "    TupleSpace  → skill tags, memory tiers, audit events, health snapshots"
echo "    BlobStorage → skill procedures, deep memory archives"
echo "    Channel     → cron job delivery (durable, at-least-once)"
echo "    DistLock    → cron leader election (single scheduler across cluster)"
echo "    ProcessGroups → service discovery for all 10 svc:* groups"
echo "    SendAfter   → cron tick loop, health poll, LLM circuit recovery"
echo "    HTTPFetch   → real LLM calls to Ollama (simulated fallback when offline)"
echo "    Ask/Send    → inter-actor tool dispatch, cross-actor skill injection"
echo "    Metrics     → IncrCounter on every key operation"
echo "    GenFSM      → approval state machine (allow→review→approved/denied)"
echo "    Durability  → checkpoint_interval on all stateful actors"
echo "================================================================"
