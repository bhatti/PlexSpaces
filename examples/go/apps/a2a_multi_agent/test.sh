#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/a2a_multi_agent_actor.wasm"
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
AUTH_HEADER=""
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

APP_ID="go-a2a-multi-agent"
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

echo "================================================================"
echo "  A2A Multi-Agent Collaboration (Go WASM)"
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
TEMP_CONFIG="$(mktemp -t a2a-multi-agent-config)"
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
echo "Step 1: Deploy A2A multi-agent application"
curl -s -X DELETE ${AUTH_HEADER:+-H "$AUTH_HEADER"} "http://localhost:$HTTP_PORT/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
sleep 1

_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_RESP=$(curl -s --connect-timeout 10 --max-time 120 -w "\n%{http_code}" \
    -X POST "http://localhost:$HTTP_PORT/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=go-a2a-multi-agent" \
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
echo -e "  ${GREEN}Deployed${NC}"
sleep 2
echo ""

# Step 2: List all agents from registry
echo "Step 2: List all registered agents from registry"
LIST_RESP=$(send_op "agent_registry" '{"op":"list_all"}' 10)
echo "  Response: $LIST_RESP" | python3 -c "
import sys, json
line = sys.stdin.read()
try:
    d = json.loads(line.split('Response: ', 1)[1])
    p = d.get('payload', d)
    agents = p.get('agents', [])
    count = p.get('count', len(agents))
    print(f'  Registered agents: {count}')
    for a in agents:
        caps = a.get('capabilities', [])
        print(f'    - {a.get(\"agent_id\",\"?\")} ({a.get(\"name\",\"?\")}): capabilities={caps}')
except Exception as e:
    print(f'  (parse error: {e}): {line[:200]}')
"
if echo "$LIST_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert p.get('status')=='ok'" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} list_all"
else
  echo -e "  ${YELLOW}WARN${NC} list_all returned unexpected: $LIST_RESP"
fi
echo ""

# Step 3: Discover agents by capability "research"
echo "Step 3: Discover agents by capability 'research'"
DISC_RESP=$(send_op "agent_registry" '{"op":"discover","capabilities":["research"]}' 10)
echo "  $DISC_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    agents = p.get('agents', [])
    print(f'  Found {len(agents)} research agent(s)')
    for a in agents:
        print(f'    - {a.get(\"agent_id\",\"?\")}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$DISC_RESP" "discover by capability research"
echo ""

# Step 4: Discover agents by capability "analysis"
echo "Step 4: Discover agents by capability 'analysis'"
DISC_ANALYSIS=$(send_op "agent_registry" '{"op":"discover","capabilities":["analysis"]}' 10)
echo "  $DISC_ANALYSIS" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    agents = p.get('agents', [])
    print(f'  Found {len(agents)} analysis agent(s)')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$DISC_ANALYSIS" "discover by capability analysis"
echo ""

# Step 5: Get agent card for research_agent
echo "Step 5: Get agent card for research_agent"
CARD_RESP=$(send_op "agent_registry" '{"op":"agent_card","agent_id":"research-1"}' 10)
echo "  $CARD_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    print(f'  Agent: {p.get(\"name\",\"?\")} ({p.get(\"agent_id\",\"?\")})')
    print(f'  Description: {p.get(\"description\",\"?\")}')
    print(f'  Capabilities: {p.get(\"capabilities\",[])}')
except Exception as e:
    print(f'  (parse error: {e})')
"
if echo "$CARD_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert 'error' not in p or p.get('status')=='ok'" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} agent_card"
else
  echo -e "  ${YELLOW}WARN${NC} agent_card: $CARD_RESP"
fi
echo ""

# Step 6: Research a topic directly
echo "Step 6: Research topic 'actors' directly via research_agent"
RESEARCH_RESP=$(send_op "research_agent" '{"op":"research","topic":"actors"}' 15)
echo "  $RESEARCH_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    findings = p.get('findings', [])
    print(f'  Topic: {p.get(\"topic\",\"?\")}')
    print(f'  Findings ({len(findings)}):')
    for f in findings[:3]:
        print(f'    - {f[:80]}')
    print(f'  Confidence: {p.get(\"confidence\",0)}')
    print(f'  Agent: {p.get(\"agent\",\"?\")}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$RESEARCH_RESP" "research topic actors"
echo ""

# Step 7: Analyze data directly
echo "Step 7: Analyze data directly via analysis_agent"
ANALYZE_RESP=$(send_op "analysis_agent" '{"op":"analyze","data":["Actors are isolated computation units","PlexSpaces uses WASM actors","Distributed systems scale horizontally"],"question":"how do distributed actors work"}' 15)
echo "  $ANALYZE_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    print(f'  Question: {p.get(\"question\",\"?\")}')
    print(f'  Key points: {len(p.get(\"key_points\",[]))}')
    print(f'  Summary: {str(p.get(\"summary\",\"\"))[:100]}')
    print(f'  Confidence: {p.get(\"confidence\",0)}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$ANALYZE_RESP" "analyze data"
echo ""

# Step 8: Write document directly
echo "Step 8: Write document directly via writer_agent"
WRITE_RESP=$(send_op "writer_agent" '{"op":"write","style":"professional","analysis":{"question":"how do distributed actors work","summary":"Distributed actors enable scalable systems.","key_points":["Actors are isolated","Messages are async","PlexSpaces runs in WASM"]}}' 15)
echo "  $WRITE_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    doc = p.get('document', '')
    print(f'  Word count: {p.get(\"word_count\",0)}')
    print(f'  Style: {p.get(\"style\",\"?\")}')
    print(f'  Document (first 150 chars): {doc[:150]}')
except Exception as e:
    print(f'  (parse error: {e})')
"
check_ok "$WRITE_RESP" "write document"
echo ""

# Step 9: Run full orchestrator workflow
echo "Step 9: Run full orchestrator workflow (task: explain distributed systems)"
echo "  (This may take a few seconds as the orchestrator delegates to all 3 specialist agents)"
ORCH_START=$(date +%s%N)
ORCH_RESP=$(send_op "orchestrator" '{"op":"workflow_run","task":"explain distributed systems","task_id":"test-task-001"}' 60)
ORCH_END=$(date +%s%N)
ORCH_WALL_MS=$(( (ORCH_END - ORCH_START) / 1000000 ))

echo "  $ORCH_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    print(f'  Task ID:  {p.get(\"task_id\",\"?\")}')
    print(f'  Status:   {p.get(\"status\",\"?\")}')
    research = p.get('research', {})
    if isinstance(research, dict):
        findings = research.get('findings', [])
        print(f'  Research: {len(findings)} finding(s)')
    analysis = p.get('analysis', {})
    if isinstance(analysis, dict):
        kp = analysis.get('key_points', [])
        print(f'  Analysis: {len(kp)} key point(s)')
    document = p.get('document', {})
    if isinstance(document, dict):
        wc = document.get('word_count', 0)
        print(f'  Document: {wc} words')
    steps = p.get('steps', {})
    print(f'  Steps completed: {list(steps.keys())}')
except Exception as e:
    print(f'  (parse error: {e})')
" 2>/dev/null || echo "  (Could not parse orchestrator response)"
echo -e "  ${CYAN}Workflow completed in ${ORCH_WALL_MS}ms${NC}"

if echo "$ORCH_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert p.get('status')=='completed' or p.get('status')=='ok'" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} orchestrator workflow run"
else
  echo -e "  ${RED}FAIL${NC} orchestrator workflow: $ORCH_RESP"
  exit 1
fi
echo ""

# Step 10: Query orchestrator status
echo "Step 10: Query orchestrator status"
STATUS_RESP=$(send_op "orchestrator" '{"op":"workflow_query","name":"status"}' 10)
echo "  $STATUS_RESP" | python3 -c "
import sys, json
try:
    d = json.loads(sys.stdin.read().strip())
    p = d.get('payload', d)
    print(f'  Task ID:  {p.get(\"task_id\",\"?\")}')
    print(f'  Status:   {p.get(\"status\",\"?\")}')
    print(f'  Progress: {p.get(\"progress\",0)}%')
except Exception as e:
    print(f'  (parse error: {e})')
"
if echo "$STATUS_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); assert 'error' not in p" 2>/dev/null; then
  echo -e "  ${GREEN}PASS${NC} orchestrator status query"
else
  echo -e "  ${YELLOW}WARN${NC} orchestrator status: $STATUS_RESP"
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
echo "Step 11: Smoke test agent_registry"
echo "  Testing list_all..."
R="$(send_actor "agent_registry" '{"op":"list_all"}' 20)"
assert_actor_ok "list_all" "$R"
echo "  ✓ list_all OK"

echo "================================================================"
echo -e "  ${GREEN}A2A Multi-Agent test passed.${NC}"
echo "================================================================"
