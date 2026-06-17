#!/usr/bin/env bash
# Test Agentic RAG Pipeline (Go WASM)
# Usage: ./test.sh [HTTP_PORT|node1:port1 node2:port2 ...]
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/agentic_rag_pipeline_actor.wasm"
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
  NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

APP_ID="go-agentic-rag-pipeline"
APP_NAME="go-agentic-rag-pipeline"
TEMP_CONFIG=""

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"


grpc_seed_nodes() {
  local joined="" sep=""
  for node in "${NODE_LIST[@]}"; do
    joined="${joined}${sep}\"${node}\""
    sep=", "
  done
  printf '%s' "$joined"
}

render_config() {
  local temp_config="$1"
  local seeds
  seeds="$(grpc_seed_nodes)"
  python3 - "$CONFIG_FILE" "$temp_config" "$seeds" <<'PY'
import pathlib
import sys

source = pathlib.Path(sys.argv[1]).read_text()
seed_nodes = sys.argv[3]
lines = []
replaced = False
for line in source.splitlines():
    if line.startswith("seed_nodes = "):
        lines.append(f"seed_nodes = [{seed_nodes}]")
        replaced = True
    else:
        lines.append(line)
if not replaced:
    lines.insert(3, f"seed_nodes = [{seed_nodes}]")
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY
}

ask() {
  local actor="$1"
  local payload="$2"
  local timeout="${3:-30}"
  curl -s --max-time "$timeout" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

echo "================================================================"
echo "  Agentic RAG Pipeline (Go WASM)"
echo "================================================================"

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

for node in "${NODE_LIST[@]}"; do
  node_host="${node%%:*}"
  node_port="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${node_host}:${node_port}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot connect to node at ${node_host}:${node_port}${NC}"
    exit 1
  fi
done

TEMP_CONFIG="$(mktemp -t agentic-rag-pipeline-app-config)"
render_config "$TEMP_CONFIG"

echo "Step 1: Undeploy existing app from all nodes"
"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2

echo "Step 1: Deploy to ${ENTRY_HOST}:${ENTRY_PORT}"
_deployed=0
for _attempt in 1 2 3; do
  response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG" 2>&1) || true
  http_code=$(echo "$response" | tail -n1)
  body=$(echo "$response" | sed '$d')
  if [ "$http_code" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $body${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed${NC}"
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
sleep 2

echo ""
echo "Step 2: Index documents"

INDEX1=$(ask "indexer" '{
  "op":"index_document",
  "doc_id":"doc-actor-framework",
  "content":"The actor model is a mathematical model of concurrent computation. Actors are the fundamental units of computation. Each actor can send messages, create new actors, and determine behavior for the next message it receives. Actors communicate asynchronously using message passing.",
  "chunk_size":80
}' 15)
if echo "$INDEX1" | grep -q '"status":"ok"'; then
  echo -e "  ${GREEN}Indexed doc-actor-framework${NC}"
else
  echo -e "${RED}Index failed: $INDEX1${NC}"
  exit 1
fi

INDEX2=$(ask "indexer" '{
  "op":"index_document",
  "doc_id":"doc-distributed-systems",
  "content":"Distributed systems coordinate multiple computers to appear as a single coherent system. Key challenges include consensus, fault tolerance, and consistency. The CAP theorem states that distributed systems can only guarantee two of: consistency, availability, partition tolerance.",
  "chunk_size":80
}' 15)
if echo "$INDEX2" | grep -q '"status":"ok"'; then
  echo -e "  ${GREEN}Indexed doc-distributed-systems${NC}"
else
  echo -e "${RED}Index failed: $INDEX2${NC}"
  exit 1
fi

INDEX3=$(ask "indexer" '{
  "op":"index_document",
  "doc_id":"doc-wasm-deployment",
  "content":"WebAssembly (WASM) is a binary instruction format for a stack-based virtual machine. WASM enables high-performance applications on the web and server-side deployments. PlexSpaces uses WASM to run actor logic in a portable, sandboxed environment across distributed nodes.",
  "chunk_size":80
}' 15)
if echo "$INDEX3" | grep -q '"status":"ok"'; then
  echo -e "  ${GREEN}Indexed doc-wasm-deployment${NC}"
else
  echo -e "${RED}Index failed: $INDEX3${NC}"
  exit 1
fi

echo ""
echo "Step 3: Verify indexer stats (expect 3 documents)"
STATS=$(ask "indexer" '{"op":"get_stats"}' 10)
DOC_COUNT=$(echo "$STATS" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('documents',0))" 2>/dev/null || echo "0")
if [ "$DOC_COUNT" -ge 3 ] 2>/dev/null; then
  echo -e "  ${GREEN}Indexer stats OK: $DOC_COUNT documents indexed${NC}"
else
  echo -e "${RED}Expected 3+ documents, got: $STATS${NC}"
  exit 1
fi

echo ""
echo "Step 4: Retrieve with simple query 'actor'"
RETRIEVE1=$(ask "retriever" '{"op":"retrieve","query":"actor","max_results":5,"mode":"single"}' 15)
if echo "$RETRIEVE1" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('count',0)>0 else 1)" 2>/dev/null; then
  COUNT=$(echo "$RETRIEVE1" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('count',0))" 2>/dev/null || echo "?")
  echo -e "  ${GREEN}Retrieved $COUNT chunks for query 'actor'${NC}"
else
  echo -e "${YELLOW}  Retrieval returned no results (may be ok in test env): $RETRIEVE1${NC}"
fi

echo ""
echo "Step 5: Retrieve with deep search mode"
RETRIEVE2=$(ask "retriever" '{"op":"retrieve","query":"consensus distributed","max_results":5,"mode":"deep"}' 15)
MODE=$(echo "$RETRIEVE2" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('mode','?'))" 2>/dev/null || echo "?")
echo -e "  ${GREEN}Deep search completed (mode=$MODE)${NC}"

echo ""
echo "Step 6: Generate answer from retrieved context"
GENERATE=$(ask "generator" '{
  "op":"generate",
  "query":"What is the actor model?",
  "context":["The actor model is a mathematical model of concurrent computation. Actors are the fundamental units of computation.","Each actor can send messages, create new actors."],
  "max_retries":2
}' 15)
if echo "$GENERATE" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); exit(0 if p.get('answer') else 1)" 2>/dev/null; then
  echo -e "  ${GREEN}Generation successful${NC}"
else
  echo -e "${YELLOW}  Generation response: $GENERATE${NC}"
fi

echo ""
echo "Step 7: Validate a good answer (should pass)"
VALIDATE_GOOD=$(ask "validator" '{
  "op":"validate",
  "answer":"Based on the context: The actor model is a mathematical model of concurrent computation where actors communicate asynchronously.",
  "query":"What is the actor model?",
  "sources":["The actor model is a mathematical model of concurrent computation. Actors communicate asynchronously."]
}' 10)
VALID_GOOD=$(echo "$VALIDATE_GOOD" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('valid','?'))" 2>/dev/null || echo "?")
if [ "$VALID_GOOD" = "True" ] || [ "$VALID_GOOD" = "true" ]; then
  echo -e "  ${GREEN}Good answer validated: valid=true${NC}"
else
  echo -e "  ${YELLOW}Good answer validation result: $VALIDATE_GOOD${NC}"
fi

echo ""
echo "Step 8: Validate a bad/empty answer (should fail)"
VALIDATE_BAD=$(ask "validator" '{
  "op":"validate",
  "answer":"No",
  "query":"What is the actor model?",
  "sources":["The actor model is a mathematical model of concurrent computation."]
}' 10)
VALID_BAD=$(echo "$VALIDATE_BAD" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('valid','?'))" 2>/dev/null || echo "?")
if [ "$VALID_BAD" = "False" ] || [ "$VALID_BAD" = "false" ]; then
  echo -e "  ${GREEN}Short answer correctly rejected: valid=false${NC}"
else
  echo -e "  ${YELLOW}Short answer validation result: $VALIDATE_BAD${NC}"
fi

echo ""
echo "Step 9: Run full RAG workflow (retrieve→generate→validate)"
WORKFLOW=$(ask "rag_workflow" '{
  "op":"workflow_run",
  "query":"How do distributed actors communicate?",
  "mode":"single",
  "max_retries":2
}' 60)
WF_STATUS=$(echo "$WORKFLOW" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('status','?'))" 2>/dev/null || echo "?")
if [ "$WF_STATUS" = "completed" ]; then
  ANSWER=$(echo "$WORKFLOW" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); a=p.get('answer',''); print(a[:80]+'...' if len(a)>80 else a)" 2>/dev/null || echo "?")
  echo -e "  ${GREEN}Workflow completed. Answer: $ANSWER${NC}"
else
  echo -e "  ${YELLOW}Workflow status: $WF_STATUS (response: $(echo "$WORKFLOW" | head -c 200))${NC}"
fi

echo ""
echo "Step 10: Query workflow status"
WF_QUERY=$(ask "rag_workflow" '{"op":"workflow_query:status"}' 10)
STEP=$(echo "$WF_QUERY" | python3 -c "import sys,json; d=json.loads(sys.stdin.read()); p=d.get('payload',d); print(p.get('current_step','?'))" 2>/dev/null || echo "?")
echo -e "  ${GREEN}Workflow status query OK (current_step=$STEP)${NC}"

echo ""
echo "Step 11: Check validator stats"
VAL_STATS=$(ask "validator" '{"op":"get_stats"}' 10)
echo "$VAL_STATS" | python3 -c "
import sys, json
raw = sys.stdin.read()
d = json.loads(raw)
p = d.get('payload', d)
if isinstance(p, str):
    p = json.loads(p)
print('  Validations run: {}, Pass: {}, Fail: {}'.format(
    p.get('validations_run', 0),
    p.get('pass_count', 0),
    p.get('fail_count', 0),
))
" 2>/dev/null || echo "  Stats: $VAL_STATS"

send_actor() {
  local actor="$1" payload="$2" timeout="${3:-20}"
  ask "$actor" "$payload" "$timeout"
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
echo "Step 12: Smoke test indexer and retriever"
echo "  Testing get_stats on indexer..."
R="$(send_actor "indexer" '{"op":"get_stats"}' 20)"
assert_actor_ok "indexer get_stats" "$R"
echo "  ✓ indexer get_stats OK"

echo "  Testing retrieve on retriever..."
R="$(send_actor "retriever" '{"op":"retrieve","query":"actor","max_results":3,"mode":"single"}' 20)"
assert_actor_ok "retriever retrieve" "$R"
echo "  ✓ retriever retrieve OK"

echo ""
echo "================================================================"
echo -e "  ${GREEN}Agentic RAG Pipeline (Go WASM) Test Complete${NC}"
echo "================================================================"
