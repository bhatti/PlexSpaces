#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$REPO_ROOT/target/examples/typescript/llm_workflow_orchestrator/llm_workflow_orchestrator_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="llm-workflow-orchestrator"
ROUTER_ACTOR="router:router-1"
CHAIN_ACTOR="chain:chain-1"
JUDGE_ACTOR="judge:judge-1"
ORCHESTRATOR_ACTOR="orchestrator:orch-1"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'


send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

extract_payload_json() {
  local raw="$1"
  RAW_RESPONSE="$raw" python3 - <<'PY'
import json, os
raw = os.environ["RAW_RESPONSE"]
data = json.loads(raw)
if not isinstance(data, dict):
    print(json.dumps({}))
    raise SystemExit(0)
payload = data.get("payload", data)
if payload is None:
    print(json.dumps({}))
    raise SystemExit(0)
if isinstance(payload, str):
    try:
        payload = json.loads(payload)
    except Exception:
        payload = {"value": payload}
if payload is None:
    payload = {}
print(json.dumps(payload))
PY
}

assert_actor_ok() {
  local step_name="$1"
  local json_raw="$2"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_raw" python3 - <<'PY'
import json, os, sys
d = json.loads(os.environ["ASSERT_JSON"])
def deep_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False:
            if obj.get("error"):
                return str(obj["error"])
            if obj.get("error_message"):
                return str(obj["error_message"])
            payload = obj.get("payload")
            if isinstance(payload, dict) and payload.get("error"):
                return str(payload["error"])
            return "request failed"
        if obj.get("error"):
            return str(obj["error"])
        for v in obj.values():
            e = deep_err(v)
            if e:
                return e
    elif isinstance(obj, list):
        for item in obj:
            e = deep_err(item)
            if e:
                return e
    return None
err = deep_err(d)
if err:
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: {err}", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step_name${NC}"
    echo "  Raw: $(echo "$json_raw" | head -c 400)"
    exit 1
  fi
}

assert_field_equals() {
  local step_name="$1" json_payload="$2" field="$3" expected="$4"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_payload" FIELD="$field" EXPECTED="$expected" python3 - <<'PY'
import json, os, sys
data = json.loads(os.environ["ASSERT_JSON"])
field = os.environ["FIELD"]
expected = os.environ["EXPECTED"]
value = data
for part in field.split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
actual = str(value) if value is not None else ""
if actual != expected:
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: field '{field}' expected '{expected}', got '{actual}'", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Field assertion failed: $step_name${NC}"
    echo "  Payload: $(echo "$json_payload" | head -c 400)"
    exit 1
  fi
}

assert_field_range() {
  local step_name="$1" json_payload="$2" field="$3" lo="$4" hi="$5"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_payload" FIELD="$field" LO="$lo" HI="$hi" python3 - <<'PY'
import json, os, sys
data = json.loads(os.environ["ASSERT_JSON"])
field = os.environ["FIELD"]
lo = float(os.environ["LO"])
hi = float(os.environ["HI"])
value = data
for part in field.split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
if value is None:
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: field '{field}' not found", file=sys.stderr)
    sys.exit(1)
v = float(value)
if not (lo <= v <= hi):
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: field '{field}' value {v} not in [{lo}, {hi}]", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Range assertion failed: $step_name${NC}"
    echo "  Payload: $(echo "$json_payload" | head -c 400)"
    exit 1
  fi
}

assert_field_not_equals() {
  local step_name="$1" json_payload="$2" field="$3" not_expected="$4"
  if ! ASSERT_STEP="$step_name" ASSERT_JSON="$json_payload" FIELD="$field" NOT_EXPECTED="$not_expected" python3 - <<'PY'
import json, os, sys
data = json.loads(os.environ["ASSERT_JSON"])
field = os.environ["FIELD"]
not_expected = os.environ["NOT_EXPECTED"]
value = data
for part in field.split("."):
    if isinstance(value, dict):
        value = value.get(part)
    else:
        value = None
        break
actual = str(value) if value is not None else ""
if actual == not_expected:
    print(f"FAIL [{os.environ['ASSERT_STEP']}]: field '{field}' should not equal '{not_expected}'", file=sys.stderr)
    sys.exit(1)
PY
  then
    echo -e "${RED}Not-equals assertion failed: $step_name${NC}"
    echo "  Payload: $(echo "$json_payload" | head -c 400)"
    exit 1
  fi
}

# -----------------------------------------------------------------------
echo "Step 1: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node with ./scripts/server.sh and re-run this test${NC}"
  exit 1
fi

# -----------------------------------------------------------------------
echo "Step 2: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1)
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo -e "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi
echo -e "${GREEN}Deployed $APP_ID${NC}"
sleep 2

# -----------------------------------------------------------------------
echo "Step 3: Router — 'summarize this' routes to summarize"
ROUTE1_RAW="$(send_actor "$ROUTER_ACTOR" '{"op":"route","content":"summarize this document for me"}' 15)"
assert_actor_ok "route_summarize" "$ROUTE1_RAW"
ROUTE1="$(extract_payload_json "$ROUTE1_RAW")"
assert_field_equals "route_summarize_field" "$ROUTE1" "route" "summarize"
echo -e "  route='summarize' ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 4: Router — long analysis query routes to analyze"
LONG_CONTENT="Please analyze and compare the following two approaches to machine learning: supervised vs unsupervised. Consider performance, data requirements, and applicability."
ROUTE2_RAW="$(send_actor "$ROUTER_ACTOR" "{\"op\":\"route\",\"content\":\"$LONG_CONTENT\"}" 15)"
assert_actor_ok "route_analyze" "$ROUTE2_RAW"
ROUTE2="$(extract_payload_json "$ROUTE2_RAW")"
assert_field_equals "route_analyze_field" "$ROUTE2" "route" "analyze"
echo -e "  route='analyze' ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 5: ChainActor — execute_chain returns 3 steps"
CHAIN_RAW="$(send_actor "$CHAIN_ACTOR" '{"op":"execute_chain","content":"The quick brown fox jumps over the lazy dog. This is a sample text for testing the chain pipeline."}' 15)"
assert_actor_ok "execute_chain" "$CHAIN_RAW"
CHAIN_PAYLOAD="$(extract_payload_json "$CHAIN_RAW")"
assert_field_equals "chain_steps" "$CHAIN_PAYLOAD" "steps_completed" "3"
echo -e "  steps_completed=3 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 6: JudgeActor — evaluate returns score in 0-10 range"
JUDGE_RAW="$(send_actor "$JUDGE_ACTOR" '{"op":"evaluate","content":"Machine learning is a subset of artificial intelligence that enables systems to learn from data.","original_query":"What is machine learning?"}' 15)"
assert_actor_ok "evaluate" "$JUDGE_RAW"
JUDGE_PAYLOAD="$(extract_payload_json "$JUDGE_RAW")"
assert_field_range "judge_score_range" "$JUDGE_PAYLOAD" "score" "0" "10"
echo -e "  score in [0,10] ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 7: ChainActor — evolve_instruction mutates prompt"
EVOLVE_RAW="$(send_actor "$CHAIN_ACTOR" '{"op":"evolve_instruction","instruction":"Explain the concept of recursion","mutations":2}' 15)"
assert_actor_ok "evolve_instruction" "$EVOLVE_RAW"
EVOLVE_PAYLOAD="$(extract_payload_json "$EVOLVE_RAW")"
assert_field_not_equals "evolved_differs" "$EVOLVE_PAYLOAD" "evolved" "Explain the concept of recursion"
assert_field_equals "mutations_applied" "$EVOLVE_PAYLOAD" "mutations_applied" "2"
echo -e "  evolved != original, mutations_applied=2 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 8: OrchestratorWorkflow — workflow_run returns completed status"
ORCH_RAW="$(send_actor "$ORCHESTRATOR_ACTOR" '{"op":"workflow_run","task":"summarize","content":"Artificial intelligence is transforming industries by enabling machines to perform tasks that traditionally required human intelligence.","max_iterations":2,"score_threshold":5.0}' 30)"
assert_actor_ok "workflow_run" "$ORCH_RAW"
ORCH_PAYLOAD="$(extract_payload_json "$ORCH_RAW")"
assert_field_equals "orch_status" "$ORCH_PAYLOAD" "status" "completed"
echo -e "  status='completed' ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 9: OrchestratorWorkflow — workflow_signal:feedback"
SIGNAL_RAW="$(send_actor "$ORCHESTRATOR_ACTOR" '{"op":"workflow_signal:feedback","content":"Great output, accepted."}' 15)"
assert_actor_ok "workflow_signal_feedback" "$SIGNAL_RAW"
echo -e "  signal:feedback ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 10: OrchestratorWorkflow — workflow_query:progress"
QUERY_RAW="$(send_actor "$ORCHESTRATOR_ACTOR" '{"op":"workflow_query:progress"}' 15)"
assert_actor_ok "workflow_query_progress" "$QUERY_RAW"
QUERY_PAYLOAD="$(extract_payload_json "$QUERY_RAW")"
assert_field_equals "query_status" "$QUERY_PAYLOAD" "status" "completed"
echo -e "  query:progress status='completed' ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 11: Stats from router, chain, judge"
ROUTER_STATS_RAW="$(send_actor "$ROUTER_ACTOR" '{"op":"get_stats"}' 15)"
assert_actor_ok "router_stats" "$ROUTER_STATS_RAW"
ROUTER_STATS="$(extract_payload_json "$ROUTER_STATS_RAW")"
assert_field_range "router_decisions" "$ROUTER_STATS" "routing_decisions" "2" "100"
echo -e "  router routing_decisions >= 2 ${GREEN}OK${NC}"

CHAIN_STATS_RAW="$(send_actor "$CHAIN_ACTOR" '{"op":"get_stats"}' 15)"
assert_actor_ok "chain_stats" "$CHAIN_STATS_RAW"
CHAIN_STATS="$(extract_payload_json "$CHAIN_STATS_RAW")"
assert_field_range "chain_steps_total" "$CHAIN_STATS" "steps_completed" "3" "1000"
echo -e "  chain steps_completed >= 3 ${GREEN}OK${NC}"

JUDGE_STATS_RAW="$(send_actor "$JUDGE_ACTOR" '{"op":"get_stats"}' 15)"
assert_actor_ok "judge_stats" "$JUDGE_STATS_RAW"
JUDGE_STATS="$(extract_payload_json "$JUDGE_STATS_RAW")"
assert_field_range "judge_evals" "$JUDGE_STATS" "evaluations_run" "1" "1000"
echo -e "  judge evaluations_run >= 1 ${GREEN}OK${NC}"

echo ""
echo -e "${GREEN}TypeScript LLM workflow orchestrator example passed${NC}"
