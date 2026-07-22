#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/resource_aware_inference_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"


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
NC='\033[0m'

APP_ID="go-resource-aware-inference"
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
  if echo "$resp" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
assert p.get('status') == 'ok' or 'error' not in p, p
" 2>/dev/null; then
    echo -e "  ${GREEN}PASS${NC} $label"
  else
    echo -e "  ${RED}FAIL${NC} $label: $resp"
    exit 1
  fi
}

check_field() {
  local resp="$1"
  local field="$2"
  local expected="$3"
  local label="$4"
  if echo "$resp" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
val = str(p.get('$field', ''))
assert val == '$expected', f'expected $field=$expected, got {val}'
" 2>/dev/null; then
    echo -e "  ${GREEN}PASS${NC} $label"
  else
    echo -e "  ${RED}FAIL${NC} $label (expected $field=$expected): $resp"
    exit 1
  fi
}

check_field_contains() {
  local resp="$1"
  local field="$2"
  local expected="$3"
  local label="$4"
  if echo "$resp" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
val = str(p.get('$field', ''))
assert '$expected' in val, f'expected $field to contain $expected, got {val}'
" 2>/dev/null; then
    echo -e "  ${GREEN}PASS${NC} $label"
  else
    echo -e "  ${RED}FAIL${NC} $label (expected $field to contain '$expected'): $resp"
    exit 1
  fi
}

echo "================================================================"
echo "  Resource-Aware Inference (Go WASM)"
echo "  HTTP Gateway: localhost:$HTTP_PORT"
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

# gRPC and HTTP share a single port
GRPC_PORT=$HTTP_PORT
TEMP_CONFIG="$(mktemp -t resource-aware-inference-config)"
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
trap 'rm -f "${APP_ZIP:-}" "${TEMP_CONFIG:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null

# Step 1: Deploy
echo "Step 1: Deploy resource-aware inference application"
curl -s -X DELETE "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_RESP=$(curl -s --connect-timeout 10 --max-time 120 -w "\n%{http_code}" \
    -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=go-resource-aware-inference" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1)
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
echo -e "  ${GREEN}Deployed${NC}"
sleep 2
echo ""

# Step 2: List models — verify 3 models returned
echo "Step 2: List models from model_registry"
LIST_RESP=$(send_op "model_registry" '{"op":"list_models"}' 10)
echo "  $LIST_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    models = p.get('models', [])
    count = p.get('count', len(models))
    print(f'  Models found: {count}')
    for m in models:
        print(f'    - {m.get(\"name\",\"?\")} (tier={m.get(\"tier\",\"?\")} cost_per_1k=\${m.get(\"cost_per_1k_tokens\",0):.3f} gpu={m.get(\"requires_gpu\",False)} mem={m.get(\"min_memory_gb\",0)}GB)')
except Exception as e:
    print(f'  (parse error: {e})')
"
if echo "$LIST_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
assert len(p.get('models', [])) == 3, f'expected 3 models, got {len(p.get(\"models\",[]))}'
" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} list_models (3 models)"
else
  echo -e "  ${RED}FAIL${NC} list_models: $LIST_RESP"
  exit 1
fi
echo ""

# Step 3: select_model with low complexity — expect small tier
echo "Step 3: select_model complexity=0.2 (expect small model)"
SEL_SMALL=$(send_op "model_registry" '{"op":"select_model","complexity":0.2,"budget_remaining":10.0,"prefer_gpu":false}' 10)
echo "  Selected: $(echo "$SEL_SMALL" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('name','?'), 'tier='+p.get('tier','?'))" 2>/dev/null)"
check_field "$SEL_SMALL" "tier" "small" "select_model small tier for complexity=0.2"
echo ""

# Step 4: select_model with high complexity + prefer_gpu — expect large tier
echo "Step 4: select_model complexity=0.8 prefer_gpu=true (expect large model)"
SEL_LARGE=$(send_op "model_registry" '{"op":"select_model","complexity":0.8,"budget_remaining":10.0,"prefer_gpu":true}' 10)
echo "  Selected: $(echo "$SEL_LARGE" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('name','?'), 'tier='+p.get('tier','?'))" 2>/dev/null)"
check_field "$SEL_LARGE" "tier" "large" "select_model large tier for complexity=0.8+gpu"
echo ""

# Step 5: Set budget for tenant "team-a" = $1.00
echo "Step 5: Set budget for tenant team-a = \$1.00"
SET_BUDGET=$(send_op "budget_manager" '{"op":"set_budget","tenant_id":"team-a","budget_usd":1.0}' 10)
check_ok "$SET_BUDGET" "set_budget team-a=\$1.00"
echo ""

# Step 6: Direct inference with small model (short prompt)
echo "Step 6: Direct inference via inference_worker_small (short prompt)"
INFER_SMALL=$(send_op "inference_worker_small" '{"op":"infer","prompt":"Hello world","max_tokens":50,"tenant_id":"team-a"}' 15)
echo "  $INFER_SMALL" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    print(f'  Model: {p.get(\"model\",\"?\")} tier={p.get(\"tier\",\"?\")} tokens={p.get(\"tokens_used\",0)} cost=\${p.get(\"cost_usd\",0):.6f} latency={p.get(\"latency_ms\",0)}ms gpu={p.get(\"gpu_used\",False)}')
    print(f'  Result: {str(p.get(\"result\",\"\"))[:80]}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$INFER_SMALL" "direct infer small model"
echo ""

# Step 7: Direct inference with large model (long prompt)
echo "Step 7: Direct inference via inference_worker_large (long prompt)"
LONG_PROMPT="This is a detailed and complex question about distributed systems, machine learning architectures, and the tradeoffs between GPU and CPU inference for large language models in production environments."
INFER_LARGE=$(send_op "inference_worker_large" "{\"op\":\"infer\",\"prompt\":\"$LONG_PROMPT\",\"max_tokens\":200,\"tenant_id\":\"team-a\"}" 15)
echo "  $INFER_LARGE" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    print(f'  Model: {p.get(\"model\",\"?\")} tier={p.get(\"tier\",\"?\")} tokens={p.get(\"tokens_used\",0)} cost=\${p.get(\"cost_usd\",0):.6f} latency={p.get(\"latency_ms\",0)}ms gpu={p.get(\"gpu_used\",False)}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$INFER_LARGE" "direct infer large model"
echo ""

# Step 8: Check budget for "team-a" after direct usage
echo "Step 8: Check budget for team-a after usage"
CHECK_BUDGET=$(send_op "budget_manager" '{"op":"check_budget","tenant_id":"team-a","estimated_cost":0.001}' 10)
echo "  $CHECK_BUDGET" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    print(f'  Budget: \${p.get(\"budget_usd\",0):.4f} Used: \${p.get(\"used_usd\",0):.6f} Remaining: \${p.get(\"remaining_usd\",0):.4f} Allowed: {p.get(\"allowed\",False)}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$CHECK_BUDGET" "check_budget team-a"
echo ""

# Step 9: routing_workflow — short prompt → expect small model routing
echo "Step 9: routing_workflow short prompt (expect small model)"
echo "  (This runs the full budget-check → model-select → infer → deduct pipeline)"
WF_SHORT_START=$(date +%s%N)
WF_SHORT=$(send_op "routing_workflow" '{"op":"workflow_run","prompt":"Hi","tenant_id":"team-a","prefer_gpu":false,"max_budget_usd":1.0}' 60)
WF_SHORT_END=$(date +%s%N)
WF_SHORT_MS=$(( (WF_SHORT_END - WF_SHORT_START) / 1000000 ))
echo "  $WF_SHORT" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    print(f'  Status: {p.get(\"status\",\"?\")} model={p.get(\"model_selected\",\"?\")} tier={p.get(\"model_tier\",\"?\")} complexity={p.get(\"complexity\",0):.2f} cost=\${p.get(\"actual_cost_usd\",0):.6f}')
except Exception as e:
    print(f'  (parse error: {e})')
"
echo -e "  ${CYAN}Workflow completed in ${WF_SHORT_MS}ms${NC}"
if echo "$WF_SHORT" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
assert p.get('status') == 'ok', f'status={p.get(\"status\")}'
assert p.get('model_tier') == 'small', f'expected small tier, got {p.get(\"model_tier\")}'
" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} routing_workflow short prompt → small tier"
else
  echo -e "  ${RED}FAIL${NC} routing_workflow short prompt: $WF_SHORT"
  exit 1
fi
echo ""

# Step 10: routing_workflow — long prompt → expect medium or large model routing
echo "Step 10: routing_workflow long prompt (expect medium or large model)"
LONG_WF_PROMPT="Explain the architectural differences between transformer-based language models and recurrent neural networks when deployed on distributed GPU clusters, including the implications for inference latency, throughput, and cost optimization strategies in a multi-tenant cloud environment."
WF_LONG_START=$(date +%s%N)
WF_LONG=$(send_op "routing_workflow" "{\"op\":\"workflow_run\",\"prompt\":\"$LONG_WF_PROMPT\",\"tenant_id\":\"team-a\",\"prefer_gpu\":true,\"max_budget_usd\":1.0}" 60)
WF_LONG_END=$(date +%s%N)
WF_LONG_MS=$(( (WF_LONG_END - WF_LONG_START) / 1000000 ))
echo "  $WF_LONG" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    print(f'  Status: {p.get(\"status\",\"?\")} model={p.get(\"model_selected\",\"?\")} tier={p.get(\"model_tier\",\"?\")} complexity={p.get(\"complexity\",0):.2f} cost=\${p.get(\"actual_cost_usd\",0):.6f}')
except Exception as e:
    print(f'  (parse error: {e})')
"
echo -e "  ${CYAN}Workflow completed in ${WF_LONG_MS}ms${NC}"
if echo "$WF_LONG" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
assert p.get('status') == 'ok', f'status={p.get(\"status\")}'
tier = p.get('model_tier', '')
assert tier in ('medium', 'large'), f'expected medium or large tier, got {tier}'
" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} routing_workflow long prompt → medium/large tier"
else
  echo -e "  ${RED}FAIL${NC} routing_workflow long prompt: $WF_LONG"
  exit 1
fi
echo ""

# Step 11: get_report from budget_manager
echo "Step 11: Budget report from budget_manager"
REPORT=$(send_op "budget_manager" '{"op":"get_report"}' 10)
echo "  $REPORT" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    report = p.get('report', [])
    print(f'  Tenants tracked: {p.get(\"tenant_count\", len(report))}')
    for entry in report:
        print(f'    tenant={entry.get(\"tenant_id\",\"?\")} budget=\${entry.get(\"budget_usd\",0):.4f} used=\${entry.get(\"used_usd\",0):.6f} remaining=\${entry.get(\"remaining_usd\",0):.4f} tokens={entry.get(\"tokens_used\",0)}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$REPORT" "get_report from budget_manager"
echo ""

# Step 12: Exceed budget test — set tiny budget then attempt inference via workflow
echo "Step 12: Exceed budget test (set budget=\$0.001, try expensive inference)"
# Set tenant "team-b" with a very small budget
SET_TINY=$(send_op "budget_manager" '{"op":"set_budget","tenant_id":"team-b","budget_usd":0.001}' 10)
check_ok "$SET_TINY" "set_budget team-b=\$0.001"

# Also consume the budget so remaining is essentially 0
CONSUME=$(send_op "budget_manager" '{"op":"deduct","tenant_id":"team-b","cost":0.001}' 10)
check_ok "$CONSUME" "deduct entire budget for team-b"

# Attempt workflow — should be rejected due to budget exceeded
BUDGET_EXCEEDED=$(send_op "routing_workflow" '{"op":"workflow_run","prompt":"This is a complex prompt that requires expensive inference","tenant_id":"team-b","prefer_gpu":true,"max_budget_usd":0.001}' 30)
echo "  Response: $(echo "$BUDGET_EXCEEDED" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('error','?'), 'remaining='+str(round(p.get('remaining_usd',0),6)))" 2>/dev/null)"
if echo "$BUDGET_EXCEEDED" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)
assert p.get('error') == 'budget_exceeded', f'expected budget_exceeded, got: {p}'
" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} budget exceeded correctly rejected"
else
  echo -e "  ${RED}FAIL${NC} expected budget_exceeded error: $BUDGET_EXCEEDED"
  exit 1
fi
echo ""

# Step 13: Query workflow cost_report
echo "Step 13: Query routing_workflow for cost report"
COST_REPORT=$(send_op "routing_workflow" '{"op":"workflow_query:cost_report"}' 15)
echo "  $COST_REPORT" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read())
    p = d.get('payload', d)
    wf_cost = p.get('workflow_total_cost_usd', 0)
    report = p.get('report', [])
    print(f'  Workflow total cost: \${wf_cost:.6f}')
    print(f'  Tenants in report: {len(report)}')
except Exception as e:
    print(f'  (parse error: {e})')
"
if echo "$COST_REPORT" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert 'error' not in p" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} workflow cost_report query"
else
  echo -e "  ${YELLOW}WARN${NC} cost_report: $COST_REPORT"
fi
echo ""

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  send_op "$actor" "$payload" "$timeout"
}

assert_actor_ok() {
  local step="$1" raw="$2"
  if ! ASSERT_STEP="$step" ASSERT_JSON="$raw" python3 - <<'PY'
import json, os, sys
raw = os.environ.get("ASSERT_JSON", "")
step = os.environ.get("ASSERT_STEP", "?")
try:
    d = json.loads(raw)
except Exception as e:
    print(f"FAIL [{step}]: invalid JSON: {e}", file=sys.stderr); sys.exit(1)
def find_err(obj):
    if isinstance(obj, dict):
        if obj.get("success") is False: return str(obj.get("error") or "request failed")
        if obj.get("status") == "error": return str(obj.get("error") or "error status")
        if obj.get("error"): return str(obj["error"])
        for v in obj.values():
            e = find_err(v)
            if e: return e
    return None
err = find_err(d)
if err: print(f"FAIL [{step}]: {err}", file=sys.stderr); sys.exit(1)
PY
  then
    echo -e "${RED}Assertion failed: $step${NC}"
    echo "  Raw: $(echo "$raw" | head -c 400)"
    exit 1
  fi
}

echo ""
echo "Step 14: Smoke test model_registry and budget_manager"
echo "  Testing list_models..."
R="$(send_actor "model_registry" '{"op":"list_models"}' 20)"
assert_actor_ok "list_models" "$R"
echo "  ✓ list_models OK"

echo "  Testing get_report..."
R="$(send_actor "budget_manager" '{"op":"get_report"}' 20)"
assert_actor_ok "get_report" "$R"
echo "  ✓ get_report OK"

echo "================================================================"
echo -e "  ${GREEN}Resource-Aware Inference test passed.${NC}"
echo "================================================================"
