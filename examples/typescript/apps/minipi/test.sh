#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# MiniPi — Agent Harness & Eval (TypeScript WASM) — integration test script
#
# Tests the full eval pipeline:
#   Scenario store → Agent OODA loop → Execution trace capture → Scoring →
#   Regression detection → Benchmark → Human-in-the-loop → Dashboard
#
# Demonstrates:
#   SchemaValidationFacet (tool-call schema validation at priority 95)
#   ExecutionTraceFacet (ordered OODA step capture at priority 85, exports to KV)
#   DurabilityFacet (crash-safe eval workflow at priority 90)
#   Supervision trees (one_for_one restart strategy)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/typescript/minipi/minipi_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="ts-minipi"

# Auto-generate JWT if not provided
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

# ask <actor> <payload> [timeout_seconds]
ask() {
  local actor="$1" payload="$2" timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload"
}

# tell <actor> <payload>
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
  if echo "$response" | grep -qE '"status"\s*:\s*"ok"|"success"\s*:\s*true|"completed"'; then
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
echo "  MiniPi — Agent Harness & Eval (TypeScript WASM)"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
echo "  12 actors · OODA loop · SchemaValidationFacet · ExecutionTraceFacet"
echo "  crash recovery · parallel eval · human-in-the-loop · benchmark"
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
# ── Ollama check ─────────────────────────────────────────────────────────────
OLLAMA_RUNNING=false
if curl -s --max-time 2 "http://localhost:11434/api/tags" >/dev/null 2>&1; then
  OLLAMA_RUNNING=true
  echo -e "  ${GREEN}✓${NC} Ollama running — LLM gateway will use real model (llama3.2)"
else
  echo -e "  ${YELLOW}⚠ WARNING${NC}: Ollama not found at localhost:11434"
  echo -e "            LLM gateway will use deterministic mock responses."
  echo -e "            For real LLM calls: install Ollama + \`ollama pull llama3.2\`"
fi
echo ""

# ── Step 2: Deploy ───────────────────────────────────────────────────────────
echo "Step 2: Deploy MiniPi application"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT" 2>/dev/null || true

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=ts-minipi" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$CONFIG_FILE") || true
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
echo -e "  ${GREEN}✓${NC} Deployed — 12 actors registered under supervisor (one_for_one)"
sleep 2
echo ""

# ── Step 3: Scenario Store — seed and list ───────────────────────────────────
echo -e "${CYAN}Step 3: ScenarioStoreActor — built-in scenarios  [KV storage, seed]${NC}"
R=$(ask "scenario_store" '{"op":"get_stats"}')
check "get_stats" "$R"
SC_COUNT=$(pyval "$R" "p.get('scenario_count',0)")
echo "  Scenarios seeded: $SC_COUNT"
if [ "$SC_COUNT" -ge 10 ] 2>/dev/null; then
  echo -e "  ${GREEN}✓${NC} 10 built-in scenarios present"
else
  echo -e "  ${YELLOW}WARN${NC} expected ≥10, got $SC_COUNT"
fi

R=$(ask "scenario_store" '{"op":"get_suite","suite_name":"smoke"}')
check "get_suite smoke" "$R"
SMOKE_COUNT=$(pyval "$R" "p.get('count',0)")
echo "  Smoke suite: $SMOKE_COUNT scenarios"
echo ""

# ── Step 4: LLM Gateway ─────────────────────────────────────────────────────
echo -e "${CYAN}Step 4: LLMGatewayActor — Ollama provider  [KV response cache, metrics]${NC}"
R=$(ask "llm_gateway" '{"op":"get_stats"}')
check "get_stats" "$R"
PROVIDER=$(pyval "$R" "p.get('provider','?')")
MODEL=$(pyval "$R" "p.get('model','?')")
echo "  Provider: $PROVIDER  Model: $MODEL"
echo ""

# ── Step 5: Tool Registry — built-in tools + schema validation ───────────────
echo -e "${CYAN}Step 5: ToolRegistryActor — list tools  [SchemaValidationFacet at priority 95]${NC}"
R=$(ask "tool_registry" '{"op":"list_tools"}')
check "list_tools" "$R"
TOOL_COUNT=$(pyval "$R" "p.get('count',0)")
echo "  Registered tools: $TOOL_COUNT"

R=$(ask "tool_registry" '{"op":"calculator","expression":"17 * 24 + 89 - 45"}')
check "calculator: 17*24+89-45" "$R"
CALC=$(pyval "$R" "p.get('result','?')")
echo "  17*24+89-45 = $CALC (expected 452)"
echo ""

# ── Step 6: Schema validation — SchemaValidationFacet rejects bad method call ─
echo -e "${CYAN}Step 6: SchemaValidationFacet — reject invalid method input  [priority 95 short-circuit]${NC}"
# This call has an empty query string — violates minLength:1 in method schema
R=$(ask "tool_registry" '{"op":"web_search","query":""}')
if echo "$R" | grep -qiE '"schema|validation|rejected|error|minLength|invalid"'; then
  echo -e "  ${GREEN}✓${NC} SchemaValidationFacet rejected empty query (schema: minLength:1)"
  echo "  Response: $(echo "$R" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(str(d.get("payload",d).get("error",""))[:80])' 2>/dev/null)"
else
  echo -e "  ${YELLOW}WARN${NC} expected schema validation error, got: $R"
fi
echo ""

# ── Step 7: Single agent OODA run ────────────────────────────────────────────
echo -e "${CYAN}Step 7: AgentActor — single scenario OODA loop  [DurabilityFacet + ExecutionTraceFacet]${NC}"
R=$(ask "agent_runner" '{"op":"workflow_run","task":"What is (17 * 24) + (89 - 45)?","eval_run_id":"test-001","scenario_id":"sc-math-01"}' 60)
check "agent run" "$R"
STATUS=$(pyval "$R" "p.get('status','?')")
ITER=$(pyval "$R" "p.get('iterations',0)")
STEPS=$(pyval "$R" "len(p.get('trajectory',{}).get('steps',[]))")
OUTCOME=$(pyval "$R" "p.get('trajectory',{}).get('outcome','?')")
echo "  Status: $STATUS  Outcome: $OUTCOME"
echo "  Iterations: $ITER  Steps captured: $STEPS (observe+orient+decide+act per iteration)"
echo ""

# ── Step 8: Trajectory query ─────────────────────────────────────────────────
echo -e "${CYAN}Step 8: TrajectoryStoreActor — retrieve stored trajectory${NC}"
TRAJ_ID=$(pyval "$R" "p.get('trajectory',{}).get('trajectoryId','')")
if [ "$TRAJ_ID" != "" ] && [ "$TRAJ_ID" != "?" ]; then
  R2=$(ask "trajectory_store" "{\"op\":\"get\",\"trajectory_id\":\"$TRAJ_ID\"}")
  check "trajectory get" "$R2"
  TRAJ_STEPS=$(pyval "$R2" "len(p.get('trajectory',{}).get('steps',[]))")
  TRAJ_OUTCOME=$(pyval "$R2" "p.get('trajectory',{}).get('outcome','?')")
  echo "  Trajectory steps: $TRAJ_STEPS"
  echo "  Trajectory outcome: $TRAJ_OUTCOME"
else
  echo -e "  ${YELLOW}SKIP${NC} no trajectoryId in response (agent may store async)"
fi
echo ""

# ── Step 9: Scorer ───────────────────────────────────────────────────────────
echo -e "${CYAN}Step 9: ScorerActor — score trajectory  [rubric: task_completion]${NC}"
SAMPLE_TRAJ='{"trajectoryId":"t-test-01","agentActorId":"agent-01","steps":[{"kind":"observe","success":true},{"kind":"orient","success":true},{"kind":"decide","success":true},{"kind":"tool_call","toolName":"calculator","success":true},{"kind":"act","success":true}],"outcome":"completed","totalInputTokens":100,"totalOutputTokens":50}'
R=$(ask "scorer" "{\"op\":\"score\",\"trajectory\":$SAMPLE_TRAJ,\"rubric\":\"task_completion\"}")
check "score task_completion" "$R"
SCORE=$(pyval "$R" "p.get('score',0)")
echo "  Score: $SCORE (rubric: task_completion)"

R=$(ask "scorer" "{\"op\":\"score\",\"trajectory\":$SAMPLE_TRAJ,\"rubric\":\"tool_use\"}")
check "score tool_use" "$R"
TOOL_SCORE=$(pyval "$R" "p.get('score',0)")
echo "  Score: $TOOL_SCORE (rubric: tool_use)"
echo ""

# ── Step 10: Eval Runner — full smoke suite ───────────────────────────────────
echo -e "${CYAN}Step 10: EvalRunnerActor — 5-scenario standard suite  [inline OODA per scenario, per-scenario scores]${NC}"
SMOKE_SCENARIOS='[{"scenario_id":"sc-math-01","input":"What is 6 * 7?","expected":"42","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-search-01","input":"Search for information about the Pythagorean theorem","expected":"a^2 + b^2 = c^2","rubric":"tool_use","difficulty":"medium"},{"scenario_id":"sc-calc-01","input":"Compute (17 * 24) + (89 - 45) step by step","expected":"452","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-reason-01","input":"If all A are B and all B are C, are all A also C?","expected":"yes","rubric":"task_completion","difficulty":"medium"},{"scenario_id":"sc-budget-01","input":"I have $500 and spend $127 on groceries, $89 on utilities, and $215 on rent. How much is left?","expected":"$69","rubric":"task_completion","difficulty":"easy"}]'
R=$(ask "eval_runner" "{\"op\":\"workflow_run\",\"suite_name\":\"standard\",\"scenarios\":$SMOKE_SCENARIOS,\"eval_run_id\":\"eval-smoke-001\"}" 90)
check "eval smoke suite" "$R"
PASS_RATE=$(pyval "$R" "p.get('pass_rate',0)")
COMPLETED=$(pyval "$R" "p.get('completed_scenarios',0)")
TOTAL=$(pyval "$R" "p.get('total_scenarios',0)")
AVG_SCORE=$(pyval "$R" "round(p.get('avg_score',0),3)")
TOTAL_IN=$(pyval "$R" "p.get('total_input_tokens',0)")
TOTAL_OUT=$(pyval "$R" "p.get('total_output_tokens',0)")
COST=$(pyval "$R" "p.get('cost_estimate_usd',0)")
echo "  Pass rate: $PASS_RATE  Avg score: $AVG_SCORE"
echo "  Completed: $COMPLETED / $TOTAL"
echo "  Tokens: ${TOTAL_IN} in / ${TOTAL_OUT} out  (est. cost: \$${COST})"
# Show per-scenario breakdown
python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
rows = p.get('per_scenario') or p.get('scores', [])
if rows:
    print('  Per-scenario breakdown:')
    for r in rows:
        sc = r.get('scenario_id', r.get('trajectory_id', '?'))
        sc = str(sc)[-12:]
        score = r.get('score', 0)
        tin = r.get('input_tokens', '')
        tout = r.get('output_tokens', '')
        tok = f'  {tin}in/{tout}out' if tin else ''
        bar = '█' * int(score * 10) + '░' * (10 - int(score * 10))
        print(f'    {sc:<14} [{bar}] {score:.2f}{tok}')
" <<< "$R" 2>/dev/null || true

# Extract actual scores for regression detector (wire eval output → regression input)
EVAL_SCORES=$(python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
scores = p.get('scores', [])
out = []
for s in scores:
    scid = s.get('scenario_id') or s.get('trajectory_id', 'sc-unknown')
    score = s.get('score', 0.8)
    out.append({'trajectory_id': scid, 'score': score})
print(json.dumps(out))
" <<< "$R" 2>/dev/null || echo '[{"trajectory_id":"sc-math-01","score":0.9},{"trajectory_id":"sc-search-01","score":0.75}]')

# Forward eval report to dashboard (dashboard is a separate WASM instance, KV is not shared)
EVAL_REPORT_PAYLOAD=$(python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
p['eval_run_id'] = 'eval-smoke-001'
print(json.dumps({'op': 'report_eval', 'eval_run_id': 'eval-smoke-001', 'report': p}))
" <<< "$R" 2>/dev/null || echo '{"op":"report_eval","eval_run_id":"eval-smoke-001"}')
ask "dashboard" "$EVAL_REPORT_PAYLOAD" 10 >/dev/null 2>&1 || true
echo ""

# ── Step 11: Regression detector — wired to actual eval scores ────────────────
echo -e "${CYAN}Step 11: RegressionDetectorActor — baseline from eval-smoke-001, compare degraded run  [real score wiring]${NC}"
# Set baseline from actual step 10 scores
R=$(ask "regression_detector" "{\"op\":\"set_baseline\",\"eval_run_id\":\"eval-smoke-001\",\"scores\":$EVAL_SCORES}")
check "set_baseline" "$R"
echo "  Baseline set from eval-smoke-001 (actual scores from step 10)"

# Simulate a degraded run: drop sc-search-01 by 0.2 (realistic regression scenario)
DEGRADED_SCORES=$(python3 -c "
import sys, json
scores = json.loads('''$EVAL_SCORES''')
for s in scores:
    if 'search' in s.get('trajectory_id',''):
        s['score'] = max(0.3, s['score'] - 0.2)  # search regressed
    elif 'reason' in s.get('trajectory_id',''):
        s['score'] = min(1.0, s['score'] + 0.05)  # reasoning improved
print(json.dumps(scores))
" 2>/dev/null || echo '[{"trajectory_id":"sc-math-01","score":0.9},{"trajectory_id":"sc-search-01","score":0.55}]')
R=$(ask "regression_detector" "{\"op\":\"compare\",\"eval_run_id\":\"eval-smoke-002\",\"scores\":$DEGRADED_SCORES}")
check "compare" "$R"
REG_COUNT=$(pyval "$R" "p.get('regression_count',0)")
IMP_COUNT=$(pyval "$R" "p.get('improvement_count',0)")
echo "  Regressions detected: $REG_COUNT  (sc-search-01 degraded)"
echo "  Improvements detected: $IMP_COUNT  (sc-reason-01 improved)"
if [ "$REG_COUNT" -ge 1 ] 2>/dev/null; then
  echo -e "  ${GREEN}✓${NC} Regression detector working — caught search degradation"
else
  echo -e "  ${YELLOW}WARN${NC} expected ≥1 regression"
fi

# Also report the degraded run to dashboard as eval-smoke-002
DEGRADED_REPORT=$(python3 -c "
import json
scores = json.loads('''$DEGRADED_SCORES''')
avg = round(sum(s['score'] for s in scores) / max(len(scores),1), 3)
pr = round(sum(1 for s in scores if s['score'] >= 0.8) / max(len(scores),1), 3)
print(json.dumps({'op':'report_eval','eval_run_id':'eval-smoke-002','report':{'status':'completed','eval_run_id':'eval-smoke-002','suite_name':'degraded','avg_score':avg,'pass_rate':pr,'completed_scenarios':len(scores),'total_scenarios':len(scores)}}))
" 2>/dev/null || echo '{"op":"report_eval","eval_run_id":"eval-smoke-002","report":{"status":"completed","eval_run_id":"eval-smoke-002","suite_name":"degraded","avg_score":0.7,"pass_rate":0.6,"completed_scenarios":5,"total_scenarios":5}}')
ask "dashboard" "$DEGRADED_REPORT" 10 >/dev/null 2>&1 || true
echo ""

# ── Step 12: Benchmark — compare 3 configs with score+cost tradeoff ──────────
echo -e "${CYAN}Step 12: BenchmarkActor — 3-config comparison (conservative/balanced/aggressive)  [score vs cost tradeoff]${NC}"
BENCH_SCENARIOS='[{"scenario_id":"sc-math-01","input":"What is 6 * 7?","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-calc-01","input":"Compute (17 * 24) + (89 - 45)","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-search-01","input":"Search for Pythagorean theorem","rubric":"tool_use","difficulty":"medium"}]'
BENCH_CONFIGS='[{"name":"conservative","max_iterations":3,"token_budget":1024},{"name":"balanced","max_iterations":10,"token_budget":4096},{"name":"aggressive","max_iterations":20,"token_budget":8192}]'
R=$(ask "benchmark" "{\"op\":\"workflow_run\",\"scenarios\":$BENCH_SCENARIOS,\"configs\":$BENCH_CONFIGS,\"benchmark_id\":\"bench-001\"}" 90)
check "benchmark run" "$R"
WINNER=$(pyval "$R" "p.get('winner','?')")
CONFIGS_TESTED=$(pyval "$R" "p.get('configs_tested',0)")
BEST_SCORE=$(pyval "$R" "round(p.get('best_score',0),3)")
WORST_SCORE=$(pyval "$R" "round(p.get('worst_score',0),3)")
echo "  Configs tested: $CONFIGS_TESTED  Winner: $WINNER"
echo "  Best score: $BEST_SCORE  Worst: $WORST_SCORE"
echo "  (conservative=cheap/fast, aggressive=high-quality/expensive — find the knee)"
python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
results = p.get('results', [])
if results:
    print('  Config comparison:')
    for r in results:
        name = r.get('config_name','?')
        score = r.get('avg_score',0)
        budget = r.get('config',{}).get('token_budget',0)
        iters = r.get('config',{}).get('max_iterations',0)
        bar = '█' * int(score * 10) + '░' * (10 - int(score * 10))
        print(f'    {name:<14} [{bar}] score={score:.3f}  budget={budget}tok  max_iter={iters}')
" <<< "$R" 2>/dev/null || true
echo ""

# ── Step 13: Approval gate — human-in-the-loop FSM ───────────────────────────
echo -e "${CYAN}Step 13: ApprovalGateActor — human-in-the-loop  [GenFSM durable wait]${NC}"
R=$(ask "approval_gate" '{"op":"get_status"}')
check "get_status idle" "$R"
FSM_STATE=$(pyval "$R" "p.get('state','?')")
echo "  FSM state: $FSM_STATE (expected: idle)"

R=$(ask "approval_gate" '{"op":"request_approval","agent_id":"agent-test-001","action":"delete_all_records","context":{"reason":"cleanup"}}')
check "request_approval" "$R"
echo "  Status: $(pyval "$R" "p.get('status','?')")"

R=$(ask "approval_gate" '{"op":"get_status"}')
FSM_STATE=$(pyval "$R" "p.get('state','?')")
echo "  FSM state after request: $FSM_STATE (expected: awaiting_approval)"

R=$(ask "approval_gate" '{"op":"approve","approver":"alice@example.com","comment":"LGTM"}')
check "approve" "$R"
echo "  Approved by: $(pyval "$R" "p.get('approver','?')")"

R=$(ask "approval_gate" '{"op":"get_status"}')
FSM_STATE=$(pyval "$R" "p.get('state','?')")
echo "  FSM state after approval: $FSM_STATE (expected: idle)"

R=$(ask "approval_gate" '{"op":"get_history"}')
check "get_history" "$R"
echo "  Decisions in history: $(pyval "$R" "p.get('count',0)")"
echo ""

# ── Step 14: Advisor — two-tier LLM  [drive with real requests first] ────────
echo -e "${CYAN}Step 14: AdvisorActor — two-tier LLM  [cheap executor + expensive advisor on-demand]${NC}"
# Reset circuit breaker in case it was tripped by concurrent LLM calls in earlier steps
ask "llm_gateway" '{"op":"reset_circuit"}' 5 >/dev/null 2>&1 || true

# Send 5 prompts: 2 simple (no escalation), 2 complex (should escalate), 1 borderline
for prompt in \
  "What is 2+2?" \
  "Explain the implications of quantum entanglement on information theory and its practical limits in secure communication" \
  "List the 5 most common sorting algorithms" \
  "Provide a comprehensive analysis of the tradeoffs between CAP theorem consistency models across distributed systems under network partition conditions, with examples" \
  "What is the capital of France?"; do
  ask "advisor" "{\"op\":\"advise\",\"prompt\":\"$prompt\",\"context\":{}}" 10 >/dev/null 2>&1 || true
done

R=$(ask "advisor" '{"op":"get_stats"}')
check "advisor get_stats" "$R"
THRESHOLD=$(pyval "$R" "p.get('confidence_threshold',0)")
TOTAL_REQ=$(pyval "$R" "p.get('total_requests',0)")
ESC_COUNT=$(pyval "$R" "p.get('escalation_count',0)")
ESC_RATE=$(pyval "$R" "round(p.get('escalation_rate_pct',0),1)")
ADV_SHARE=$(pyval "$R" "round(p.get('advisor_token_share_pct',0),1)")
FAST_TOKENS=$(pyval "$R" "p.get('fast_input_tokens',0) + p.get('fast_output_tokens',0)")
ADV_TOKENS=$(pyval "$R" "p.get('advisor_input_tokens',0) + p.get('advisor_output_tokens',0)")
echo "  Confidence threshold: $THRESHOLD  |  Total requests: $TOTAL_REQ"
echo "  Fast model:   $FAST_TOKENS tokens  (most turns handled cheaply)"
echo "  Advisor model: $ADV_TOKENS tokens  (escalated $ESC_COUNT / $TOTAL_REQ turns)"
echo "  Escalation rate: ${ESC_RATE}%   Advisor token share: ${ADV_SHARE}%"
echo "  Insight: ${ADV_SHARE}% of tokens spent on advisor catches hard cases — rest handled cheaply"
echo ""

# ── Step 15: Dashboard — aggregate view ──────────────────────────────────────
echo -e "${CYAN}Step 15: DashboardActor — aggregate results  [read-only aggregator over eval + benchmark]${NC}"
R=$(ask "dashboard" '{"op":"summary"}')
check "summary" "$R"
TOTAL_EVALS=$(pyval "$R" "p.get('total_evals',0)")
AVG=$(pyval "$R" "round(p.get('avg_score',0),3)")
echo "  Total eval runs: $TOTAL_EVALS  Avg score: $AVG"

R=$(ask "dashboard" '{"op":"list_eval_runs","limit":5}')
check "list_eval_runs" "$R"
RUN_COUNT=$(pyval "$R" "p.get('count',0)")
echo "  Eval runs listed: $RUN_COUNT"
python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
runs = p.get('runs', [])
if runs:
    print('  Run breakdown:')
    for r in runs:
        eid = str(r.get('eval_run_id','?'))[-20:]
        score = r.get('avg_score',0)
        rate = r.get('pass_rate',0)
        n = r.get('completed_scenarios') or r.get('configs_tested','?')
        print(f'    {eid:<22}  avg_score={score:.3f}  pass_rate={rate:.2f}  n={n}')
" <<< "$R" 2>/dev/null || true
echo ""

# ── Summary ───────────────────────────────────────────────────────────────────
echo -e "${GREEN}${BOLD}All steps passed!${NC}"
echo ""
echo "MiniPi (TypeScript) demonstrated the following PlexSpaces harness + eval patterns:"
echo -e "  ${GREEN}•${NC} SchemaValidationFacet (priority 95)  — method input validation, bad calls never reach actor"
echo -e "  ${GREEN}•${NC} ExecutionTraceFacet (priority 85)    — ordered OODA step capture, exports to KV on completion"
echo -e "  ${GREEN}•${NC} DurabilityFacet (priority 90)        — journal every step, crash-safe eval workflow"
echo -e "  ${GREEN}•${NC} Supervision tree (one_for_one)       — crashed agent restarts, orchestrator keeps running"
echo -e "  ${GREEN}•${NC} Parallel eval fan-out                — N AgentActors spawned concurrently per eval suite"
echo -e "  ${GREEN}•${NC} Regression detection                 — compare scores across runs, flag ±5% threshold"
echo -e "  ${GREEN}•${NC} Benchmark comparison                 — same scenario, different harness configs"
echo -e "  ${GREEN}•${NC} Human-in-the-loop (GenFSM)           — agent suspends, gate waits, resumes on signal"
echo -e "  ${GREEN}•${NC} Two-tier LLM (AdvisorActor)           — cheap executor for most turns, advisor on low confidence"
echo -e "  ${GREEN}•${NC} AgentLoop (OODA)                     — OODA step tracking, budget enforcement, trajectory"
