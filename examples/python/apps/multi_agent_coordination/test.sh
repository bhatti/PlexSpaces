#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Multi-Agent Coordination (Python WASM) — integration test script
# Tests all 10 coordination patterns across 8 actors.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
WASM_FILE="$SCRIPT_DIR/multi_agent_coordination_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"
APP_ID="py-multi-agent-coordination"

COORDINATOR="coordinator:coordinator"
RESEARCH="research:research"
ANALYSIS="analysis:analysis"
VERIFIER="verifier:verifier"
SYNTHESIZER="synthesizer:synthesizer"
BENCHMARK="benchmark:benchmark"
AUDIT="audit:audit"
FSM="coordination_fsm:coordination_fsm"

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# Auto-generate JWT if not provided
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

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  curl -s --max-time "$timeout" -X POST \
    "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
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
echo "Step 0: Build WASM"
bash "$SCRIPT_DIR/build.sh"
echo ""

trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
  echo -e "${RED}Start node with ./scripts/server.sh and re-run this test${NC}"
  exit 1
fi

# -----------------------------------------------------------------------
echo "Step 0b: Deploy"
"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 2
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_ID" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
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
echo ""
echo "Step 1: FSM — initial state is idle"
FSM_RAW="$(send_actor "$FSM" '{"op":"get_state"}' 15)"
assert_actor_ok "fsm_init" "$FSM_RAW"
FSM_P="$(extract_payload_json "$FSM_RAW")"
assert_field_equals "fsm_idle" "$FSM_P" "current_state" "idle"
echo -e "  current_state=idle ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 2: Capability Discovery — research, analysis, verifier respond"
R_RAW="$(send_actor "$RESEARCH" '{"op":"get_stats"}' 15)"
assert_actor_ok "research_stats" "$R_RAW"
A_RAW="$(send_actor "$ANALYSIS" '{"op":"get_stats"}' 15)"
assert_actor_ok "analysis_stats" "$A_RAW"
V_RAW="$(send_actor "$VERIFIER" '{"op":"get_stats"}' 15)"
assert_actor_ok "verifier_stats" "$V_RAW"
echo -e "  All 3 agents registered ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 3: Blackboard — write finding (SQL injection)"
F1_RAW="$(send_actor "$RESEARCH" '{"op":"research","topic":"SQL injection in user input handlers"}' 15)"
assert_actor_ok "research_sql" "$F1_RAW"
F1="$(extract_payload_json "$F1_RAW")"
assert_field_not_equals "finding_id_exists" "$F1" "finding_id" ""
echo -e "  finding written ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 4: Blackboard — write 2 more findings"
F2_RAW="$(send_actor "$RESEARCH" '{"op":"research","topic":"Authentication bypass via JWT token manipulation"}' 15)"
assert_actor_ok "research_auth" "$F2_RAW"
F3_RAW="$(send_actor "$RESEARCH" '{"op":"research","topic":"Cross-site scripting in HTML templates"}' 15)"
assert_actor_ok "research_xss" "$F3_RAW"
echo -e "  2 more findings written ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 5: Blackboard read — analyze finds >= 3 findings"
AN_RAW="$(send_actor "$ANALYSIS" '{"op":"analyze"}' 15)"
assert_actor_ok "analyze" "$AN_RAW"
AN="$(extract_payload_json "$AN_RAW")"
assert_field_range "finding_count" "$AN" "finding_count" "3" "100"
echo -e "  finding_count >= 3 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 6: Dynamic Task Delegation — write 5 tasks, claim all, 6th returns null"
PREP_RAW="$(send_actor "$RESEARCH" '{"op":"prepare_tasks","count":5,"prefix":"delegation"}' 15)"
assert_actor_ok "prepare_tasks" "$PREP_RAW"
PREP_P="$(extract_payload_json "$PREP_RAW")"
assert_field_equals "tasks_written" "$PREP_P" "tasks_written" "5"
BATCH_KEY=$(echo "$PREP_P" | python3 -c "import json,sys; print(json.load(sys.stdin).get('batch_key',''))")

CLAIMED=0
for i in 1 2 3 4 5; do
  C_RAW="$(send_actor "$RESEARCH" "{\"op\":\"claim_task\",\"batch_key\":\"$BATCH_KEY\"}" 15)"
  C="$(extract_payload_json "$C_RAW")"
  IS_CLAIMED=$(echo "$C" | python3 -c "import json,sys; d=json.load(sys.stdin); print('yes' if d.get('claimed') else 'no')")
  if [ "$IS_CLAIMED" = "yes" ]; then
    CLAIMED=$((CLAIMED + 1))
  fi
done
C6_RAW="$(send_actor "$RESEARCH" "{\"op\":\"claim_task\",\"batch_key\":\"$BATCH_KEY\"}" 15)"
C6="$(extract_payload_json "$C6_RAW")"
assert_field_equals "6th_claim_null" "$C6" "claimed" "False"
echo -e "  claimed=$CLAIMED/5, 6th=null ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 7: Generator-Verifier + Pipeline — full coordinator workflow"
WF_RAW="$(send_actor "$COORDINATOR" '{"op":"workflow_run","task":"Analyze authentication bypass vulnerabilities in login endpoints"}' 60)"
assert_actor_ok "workflow_run" "$WF_RAW"
WF="$(extract_payload_json "$WF_RAW")"
assert_field_equals "wf_status" "$WF" "status" "completed"
assert_field_not_equals "wf_report" "$WF" "report" ""
echo -e "  status=completed, report non-empty ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 8: Pipeline verified — FSM reached complete"
FSM2_RAW="$(send_actor "$FSM" '{"op":"get_state"}' 15)"
assert_actor_ok "fsm_complete" "$FSM2_RAW"
FSM2="$(extract_payload_json "$FSM2_RAW")"
assert_field_equals "fsm_complete_state" "$FSM2" "current_state" "complete"
echo -e "  FSM current_state=complete ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 9: Pub-Sub — audit log has events"
AUDIT_RAW="$(send_actor "$AUDIT" '{"op":"get_audit_log","limit":20}' 15)"
assert_actor_ok "audit_log" "$AUDIT_RAW"
AUDIT_P="$(extract_payload_json "$AUDIT_RAW")"
assert_field_range "audit_count" "$AUDIT_P" "total_count" "3" "1000"
echo -e "  total_count >= 3 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 10: Consensus/Voting — 3 votes on proposal"
V1_RAW="$(send_actor "$VERIFIER" '{"op":"vote","proposal_id":"sec-review-1","voter_id":"v1","analysis":{"severity":"high"}}' 15)"
assert_actor_ok "vote_v1" "$V1_RAW"
V1="$(extract_payload_json "$V1_RAW")"
assert_field_equals "v1_decision" "$V1" "decision" "approve"

V2_RAW="$(send_actor "$VERIFIER" '{"op":"vote","proposal_id":"sec-review-1","voter_id":"v2","analysis":{"severity":"high"}}' 15)"
assert_actor_ok "vote_v2" "$V2_RAW"

V3_RAW="$(send_actor "$VERIFIER" '{"op":"vote","proposal_id":"sec-review-1","voter_id":"v3","analysis":{"severity":"high"}}' 15)"
assert_actor_ok "vote_v3" "$V3_RAW"
echo -e "  3 votes cast (high severity → all approve) ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 11: Verify votes in stats"
VS_RAW="$(send_actor "$VERIFIER" '{"op":"get_stats"}' 15)"
VS="$(extract_payload_json "$VS_RAW")"
assert_field_range "votes_cast" "$VS" "votes_cast" "3" "100"
echo -e "  votes_cast >= 3 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 12: Veto Protocol — verify with low confidence triggers veto"
VETO_RAW="$(send_actor "$VERIFIER" '{"op":"verify","analysis_id":"a-weak","summary":"unverified claim","severity":"low","confidence":0.1}' 15)"
assert_actor_ok "veto_verify" "$VETO_RAW"
VETO_P="$(extract_payload_json "$VETO_RAW")"
assert_field_equals "veto_issued" "$VETO_P" "veto_issued" "True"
echo -e "  veto_issued=true ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 13: Veto effect — synthesizer excludes vetoed analyses"
SYNTH_RAW="$(send_actor "$SYNTHESIZER" '{"op":"synthesize"}' 15)"
assert_actor_ok "synthesize" "$SYNTH_RAW"
SYNTH="$(extract_payload_json "$SYNTH_RAW")"
assert_field_range "vetoed_count" "$SYNTH" "vetoed_count" "1" "100"
echo -e "  vetoed_count >= 1 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 14: Barrier benchmark"
BARRIER_RAW="$(send_actor "$BENCHMARK" '{"op":"run_pattern_benchmark","pattern":"barrier","iterations":5}' 15)"
assert_actor_ok "barrier_bench" "$BARRIER_RAW"
BARRIER="$(extract_payload_json "$BARRIER_RAW")"
assert_field_range "barrier_avg" "$BARRIER" "avg_ms" "0" "10000"
echo -e "  barrier benchmark avg_ms > 0 ${GREEN}OK${NC}"

# -----------------------------------------------------------------------
echo "Step 15: Full benchmark — all 10 patterns"
BENCH_RAW="$(send_actor "$BENCHMARK" '{"op":"run_all_benchmarks","iterations":10}' 30)"
assert_actor_ok "all_benchmarks" "$BENCH_RAW"
BENCH="$(extract_payload_json "$BENCH_RAW")"
assert_field_range "patterns_tested" "$BENCH" "patterns_tested" "10" "10"
echo -e "  all 10 patterns benchmarked ${GREEN}OK${NC}"

# Print benchmark table
echo ""
echo "=== Multi-Agent Coordination Pattern Benchmarks ==="
BENCH_TABLE="$BENCH" python3 - <<'PY'
import json, os
data = json.loads(os.environ["BENCH_TABLE"])
results = data.get("results", [])
print(f"{'Pattern':<25} {'Ops':>5} {'Avg(ms)':>8} {'p50(ms)':>8} {'p95(ms)':>8} {'TPS':>8}")
for r in results:
    if "error" in r:
        print(f"{r['pattern']:<25} ERROR: {r['error']}")
    else:
        print(f"{r['pattern']:<25} {r.get('iterations',0):>5} {r.get('avg_ms',0):>8.1f} {r.get('p50_ms',0):>8.1f} {r.get('p95_ms',0):>8.1f} {r.get('tps',0):>8.0f}")
PY

echo ""
echo -e "${GREEN}Python Multi-Agent Coordination example — all 15 steps passed${NC}"
