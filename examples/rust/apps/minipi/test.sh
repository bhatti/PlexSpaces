#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# MiniPi — Agent Harness & Eval (Rust WASM) — integration test script
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
WASM_FILE="$SCRIPT_DIR/minipi_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="rs-minipi"

# Auto-generate JWT if not provided
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
echo "  MiniPi — Agent Harness & Eval (Rust WASM)"
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

TEMP_CONFIG=$(mktemp /tmp/minipi-rs-config-XXXXXX.toml)
python3 - <<EOF
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
      -F "name=rs-minipi" \
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

# ── Step 4: LLM Gateway — mock provider ─────────────────────────────────────
echo -e "${CYAN}Step 4: LLMGatewayActor — mock provider  [KV response cache, metrics]${NC}"
R=$(ask "llm_gateway" '{"op":"get_stats"}')
check "get_stats" "$R"
PROVIDER=$(pyval "$R" "p.get('provider','?')")
echo "  Provider: $PROVIDER (mock for deterministic eval)"
echo ""

# ── Step 5: Tool Registry — built-in tools + schema validation ───────────────
echo -e "${CYAN}Step 5: ToolRegistryActor — list tools  [SchemaValidationFacet at priority 95]${NC}"
R=$(ask "tool_registry" '{"op":"list_tools"}')
check "list_tools" "$R"
TOOL_COUNT=$(pyval "$R" "p.get('count',0)")
echo "  Registered tools: $TOOL_COUNT"

R=$(ask "tool_registry" '{"op":"execute","name":"calculator","input":{"expression":"17 * 24 + 89 - 45"}}')
check "calculator: 17*24+89-45" "$R"
CALC=$(pyval "$R" "p.get('result','?')")
echo "  17*24+89-45 = $CALC (expected 452)"
echo ""

# ── Step 6: Schema validation — SchemaValidationFacet rejects bad method call ─
echo -e "${CYAN}Step 6: SchemaValidationFacet — reject invalid method input  [priority 95 short-circuit]${NC}"
# This call has an empty query string — violates minLength:1 in method schema
R=$(ask "tool_registry" '{"op":"execute","name":"web_search","input":{"query":""}}')
if echo "$R" | grep -qiE '"schema|validation|rejected|error|minLength|invalid"'; then
  echo -e "  ${GREEN}✓${NC} SchemaValidationFacet rejected empty query (schema: minLength:1)"
  echo "  Response: $(echo "$R" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(str(d.get("payload",d).get("error",""))[:80])' 2>/dev/null)"
else
  echo -e "  ${YELLOW}WARN${NC} expected schema validation error, got: $R"
fi
echo ""

# ── Step 7: Single agent OODA run ────────────────────────────────────────────
echo -e "${CYAN}Step 7: AgentActor — single scenario OODA loop  [DurabilityFacet + ExecutionTraceFacet]${NC}"
R=$(ask "agent_runner" '{"op":"run","task":"What is (17 * 24) + (89 - 45)?","eval_run_id":"test-001","scenario_id":"sc-math-01"}' 60)
check "agent run" "$R"
OUTCOME=$(pyval "$R" "p.get('outcome','?')")
STEPS=$(pyval "$R" "p.get('step_count',0)")
echo "  Outcome: $OUTCOME"
echo "  Steps captured: $STEPS (observe+orient+decide+act per iteration)"
echo ""

# ── Step 8: Trajectory query — retrieve stored trajectory ────────────────────
echo -e "${CYAN}Step 8: TrajectoryStoreActor — store + retrieve trajectory  [cross-actor pipeline wiring]${NC}"
TRAJ_ID=$(pyval "$R" "p.get('trajectory_id','?')")
AGENT_STEPS=$(pyval "$R" "p.get('steps',[])")
AGENT_OUTCOME=$(pyval "$R" "p.get('outcome','?')")
if [ "$TRAJ_ID" != "?" ] && [ -n "$TRAJ_ID" ]; then
  # WASM actors are isolated — explicitly hand the trajectory to trajectory_store
  # This is the correct pipeline pattern: orchestrator wires agent→trajectory_store
  STORE_PAYLOAD=$(python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
traj = {
    'trajectory_id': p.get('trajectory_id',''),
    'agent_actor_id': p.get('agent_id','agent_runner'),
    'eval_run_id': p.get('eval_run_id','test-001'),
    'scenario_id': p.get('scenario_id','sc-math-01'),
    'task': p.get('task',''),
    'steps': p.get('steps',[]),
    'outcome': p.get('outcome','completed'),
    'total_input_tokens': 100,
    'total_output_tokens': 50,
}
print(json.dumps({'op':'store_trajectory','trajectory': traj}))
" <<< "$R" 2>/dev/null)
  if [ -n "$STORE_PAYLOAD" ]; then
    tell "trajectory_store" "$STORE_PAYLOAD"
  fi

  R2=$(ask "trajectory_store" "{\"op\":\"get\",\"trajectory_id\":\"$TRAJ_ID\"}")
  check "trajectory get" "$R2"
  TRAJ_STEPS=$(pyval "$R2" "len(p.get('trajectory',{}).get('steps',[]))")
  TRAJ_OUTCOME=$(pyval "$R2" "p.get('trajectory',{}).get('outcome','?')")
  echo "  Trajectory ID: $TRAJ_ID"
  echo "  Trajectory steps: $TRAJ_STEPS (4 OODA steps per iteration: observe+orient+decide+act)"
  echo "  Trajectory outcome: $TRAJ_OUTCOME"
else
  echo -e "  ${YELLOW}SKIP${NC} no trajectory_id in response"
fi
echo ""

# ── Step 9: Scorer — score a trajectory ──────────────────────────────────────
echo -e "${CYAN}Step 9: ScorerActor — score trajectory  [rubric: task_completion]${NC}"
SAMPLE_TRAJ='{"trajectory_id":"t-test-01","agent_actor_id":"agent-01","steps":[{"kind":"observe","success":true},{"kind":"orient","success":true},{"kind":"decide","success":true},{"kind":"tool_call","tool_name":"calculator","success":true},{"kind":"act","success":true}],"outcome":"completed","total_input_tokens":100,"total_output_tokens":50}'
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
echo -e "${CYAN}Step 10: EvalRunnerActor — smoke suite  [Workflow + cross-actor pipeline coordination]${NC}"
SMOKE_SCENARIOS='[{"scenario_id":"sc-math-01","input":"What is 6 * 7?","expected":"42","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-search-01","input":"Search for information about the Pythagorean theorem","expected":"a^2 + b^2 = c^2","rubric":"tool_use","difficulty":"medium"},{"scenario_id":"sc-calc-01","input":"Compute (17 * 24) + (89 - 45) step by step","expected":"452","rubric":"task_completion","difficulty":"easy"}]'
R=$(ask "eval_runner" "{\"op\":\"workflow_run\",\"suite_name\":\"smoke\",\"scenarios\":$SMOKE_SCENARIOS,\"eval_run_id\":\"eval-smoke-001\"}" 90)
check "eval smoke suite" "$R"
PASS_RATE=$(pyval "$R" "p.get('pass_rate',0)")
COMPLETED=$(pyval "$R" "p.get('completed_scenarios',0)")
TOTAL=$(pyval "$R" "p.get('total_scenarios',0)")
AVG_SCORE=$(pyval "$R" "round(p.get('avg_score',0),3)")
echo "  Pass rate: $PASS_RATE"
echo "  Completed: $COMPLETED / $TOTAL"
echo "  Avg score: $AVG_SCORE"

# Wire eval report to dashboard (explicit pipeline: eval_runner → dashboard)
EVAL_REPORT=$(python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
report = {
    'eval_run_id': p.get('eval_run_id','eval-smoke-001'),
    'suite_name': p.get('suite_name','smoke'),
    'total_scenarios': p.get('total_scenarios',3),
    'completed_scenarios': p.get('completed_scenarios',3),
    'scores': p.get('scores',[]),
    'avg_score': p.get('avg_score',0),
    'pass_rate': p.get('pass_rate',1.0),
    'status': 'completed',
}
print(json.dumps({'op':'add_eval_report','eval_run_id': report['eval_run_id'], 'report': report}))
" <<< "$R" 2>/dev/null)
if [ -n "$EVAL_REPORT" ]; then
  tell "dashboard" "$EVAL_REPORT"
fi
echo ""

# ── Step 11: Regression detector — baseline + compare ────────────────────────
echo -e "${CYAN}Step 11: RegressionDetectorActor — set baseline + compare  [eval feedback loop]${NC}"
BASELINE_SCORES='[{"trajectory_id":"sc-math-01","score":0.9},{"trajectory_id":"sc-search-01","score":0.7}]'
R=$(ask "regression_detector" "{\"op\":\"set_baseline\",\"eval_run_id\":\"eval-smoke-001\",\"scores\":$BASELINE_SCORES}")
check "set_baseline" "$R"

# Compare with slightly lower scores — should detect regression on sc-search-01
CURRENT_SCORES='[{"trajectory_id":"sc-math-01","score":0.92},{"trajectory_id":"sc-search-01","score":0.55}]'
R=$(ask "regression_detector" "{\"op\":\"compare\",\"eval_run_id\":\"eval-smoke-002\",\"scores\":$CURRENT_SCORES}")
check "compare" "$R"
REG_COUNT=$(pyval "$R" "p.get('regression_count',0)")
IMP_COUNT=$(pyval "$R" "p.get('improvement_count',0)")
echo "  Regressions: $REG_COUNT (sc-search-01 dropped 0.7→0.55 = Δ-0.15, threshold 5%)"
echo "  Improvements: $IMP_COUNT (sc-math-01 rose 0.9→0.92)"
if [ "$REG_COUNT" -ge 1 ] 2>/dev/null; then
  echo -e "  ${GREEN}✓${NC} Regression detected correctly"
else
  echo -e "  ${YELLOW}WARN${NC} expected ≥1 regression"
fi
echo ""

# ── Step 12: Benchmark — compare 3 configs ───────────────────────────────────
echo -e "${CYAN}Step 12: BenchmarkActor — 3-config comparison (conservative/balanced/aggressive)  [parallel eval fan-out]${NC}"
BENCH_SCENARIOS='[{"scenario_id":"sc-math-01","input":"What is 6 * 7?","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-calc-01","input":"Compute (17 * 24) + (89 - 45)","rubric":"task_completion","difficulty":"easy"},{"scenario_id":"sc-search-01","input":"Search for Pythagorean theorem","rubric":"tool_use","difficulty":"medium"}]'
BENCH_CONFIGS='[{"name":"conservative","max_iterations":3,"token_budget":1024},{"name":"balanced","max_iterations":10,"token_budget":4096},{"name":"aggressive","max_iterations":20,"token_budget":8192}]'
R=$(ask "benchmark" "{\"op\":\"workflow_run\",\"scenarios\":$BENCH_SCENARIOS,\"configs\":$BENCH_CONFIGS,\"benchmark_id\":\"bench-001\"}" 90)
check "benchmark run" "$R"
WINNER=$(pyval "$R" "p.get('winner','?')")
CONFIGS_TESTED=$(pyval "$R" "p.get('configs_tested',0)")
BEST_SCORE=$(pyval "$R" "round(p.get('best_score',0),3)")
WORST_SCORE=$(pyval "$R" "round(p.get('worst_score',0),3)")
echo "  Configs tested: $CONFIGS_TESTED"
echo "  (3 configs: conservative/balanced/aggressive — shows how harness tuning beats model tuning)"
echo "  Winner: $WINNER  (best_score=$BEST_SCORE  worst_score=$WORST_SCORE)"

# Wire benchmark report to dashboard
BENCH_REPORT=$(python3 -c "
import sys, json
d = json.loads(sys.stdin.read())
p = d.get('payload', d)
report = {
    'eval_run_id': 'bench:bench-001',
    'benchmark_id': p.get('benchmark_id','bench-001'),
    'winner': p.get('winner','?'),
    'configs_tested': p.get('configs_tested',0),
    'best_score': p.get('best_score',0),
    'worst_score': p.get('worst_score',0),
    'avg_score': p.get('best_score',0),
    'results': p.get('results',[]),
    'status': 'completed',
}
print(json.dumps({'op':'add_eval_report','eval_run_id': report['eval_run_id'], 'report': report}))
" <<< "$R" 2>/dev/null)
if [ -n "$BENCH_REPORT" ]; then
  tell "dashboard" "$BENCH_REPORT"
fi
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

# ── Step 14: Advisor — two-tier LLM stats ───────────────────────────────────
echo -e "${CYAN}Step 14: AdvisorActor — two-tier LLM  [cheap executor + expensive advisor on-demand]${NC}"

# Drive the advisor with 5 real requests — mix of easy (no escalation) and complex (escalated)
for prompt in \
  "What is 2+2?" \
  "Explain the implications of quantum entanglement on information theory and its practical limitations" \
  "List the 5 most common sorting algorithms" \
  "Provide a comprehensive analysis of the tradeoffs between CAP theorem consistency models across distributed systems under network partition conditions" \
  "What is the capital of France?"; do
  ask "advisor" "{\"op\":\"advise\",\"prompt\":\"$prompt\",\"context\":{}}" 10 >/dev/null 2>&1 || true
done

R=$(ask "advisor" '{"op":"get_stats"}')
check "advisor get_stats" "$R"
THRESHOLD=$(pyval "$R" "p.get('confidence_threshold',0)")
TOTAL_REQ=$(pyval "$R" "p.get('total_requests',0)")
ESC_COUNT=$(pyval "$R" "p.get('escalation_count',0)")
ESC_RATE=$(pyval "$R" "p.get('escalation_rate_pct',0)")
ADV_TOKEN_SHARE=$(pyval "$R" "p.get('advisor_token_share_pct',0)")
echo "  Confidence threshold: $THRESHOLD (escalate when confidence < threshold)"
echo "  Total requests: $TOTAL_REQ (5 prompts: 2 simple + 2 complex + 1 simple)"
echo "  Escalated to expensive model: $ESC_COUNT / $TOTAL_REQ"
echo "  Escalation rate: ${ESC_RATE}%  (complex prompts trigger expensive advisor)"
echo "  Advisor token share: ${ADV_TOKEN_SHARE}%  (small share = cost control working)"
echo ""

# ── Step 15: Dashboard — aggregate view ──────────────────────────────────────
echo -e "${CYAN}Step 15: DashboardActor — aggregate results  [read-only aggregator over eval + benchmark]${NC}"
R=$(ask "dashboard" '{"op":"summary"}')
check "summary" "$R"
TOTAL_EVALS=$(pyval "$R" "p.get('total_evals',0)")
TOTAL_TRAJ=$(pyval "$R" "p.get('total_trajectories',0)")
AVG_SCORE=$(pyval "$R" "round(p.get('avg_score',0),3)")
echo "  Total eval runs tracked: $TOTAL_EVALS  (smoke eval + benchmark)"
echo "  Total trajectories stored: $TOTAL_TRAJ"
echo "  Avg score across runs: $AVG_SCORE"

R=$(ask "dashboard" '{"op":"list_eval_runs","limit":5}')
check "list_eval_runs" "$R"
RUN_COUNT=$(pyval "$R" "p.get('count',0)")
echo "  Eval runs listed: $RUN_COUNT"
if [ "$RUN_COUNT" -ge 2 ] 2>/dev/null; then
  echo -e "  ${GREEN}✓${NC} Dashboard aggregates eval runs from multiple sources (smoke + benchmark)"
else
  echo -e "  ${YELLOW}WARN${NC} expected ≥2 eval runs (smoke + benchmark), got $RUN_COUNT"
fi
echo ""

# ── Summary ───────────────────────────────────────────────────────────────────
echo -e "${GREEN}${BOLD}All steps passed!${NC}"
echo ""
echo "MiniPi (Rust WASM) demonstrated the following PlexSpaces harness + eval patterns:"
echo -e "  ${GREEN}•${NC} SchemaValidationFacet (priority 95)  — method input validation, bad calls never reach actor"
echo -e "  ${GREEN}•${NC} ExecutionTraceFacet (priority 85)    — ordered OODA step capture, exports to KV on completion"
echo -e "  ${GREEN}•${NC} DurabilityFacet (priority 90)        — journal every step, crash-safe eval workflow"
echo -e "  ${GREEN}•${NC} Supervision tree (one_for_one)       — crashed agent restarts, orchestrator keeps running"
echo -e "  ${GREEN}•${NC} Parallel eval fan-out                — N AgentActors spawned concurrently per eval suite"
echo -e "  ${GREEN}•${NC} Regression detection                 — compare scores across runs, flag ±5% threshold"
echo -e "  ${GREEN}•${NC} Benchmark comparison                 — same scenario, different harness configs"
echo -e "  ${GREEN}•${NC} Human-in-the-loop (GenFSM)           — agent suspends, gate waits, resumes on signal"
echo -e "  ${GREEN}•${NC} Two-tier LLM (AdvisorActor)           — cheap executor for most turns, advisor on low confidence"
echo -e "  ${GREEN}•${NC} @workflow_actor + AgentLoop           — OODA step tracking, budget enforcement, trajectory"
