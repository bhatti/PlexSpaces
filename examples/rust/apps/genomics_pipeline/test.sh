#!/bin/bash
# Test Genomics Pipeline WASM app: deploy, one run (single-actor on entry node; no leader/worker distribution), query metrics.
# Usage:
#   ./test.sh [HTTP_PORT]                        # single node at localhost:HTTP_PORT (default 8092)
#   ./test.sh host1:port1 host2:port2 ...         # multi-node: deploy to ALL, ConnectNodes, send ONE run to entry (first) node
#   ./test.sh "host1:port1 host2:port2 ..."      # same (quoted list)
# Prerequisites: Start server(s), e.g. ./scripts/server.sh 8091 and ./scripts/server.sh 8093 for two nodes (HTTP 8092, 8094).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/genomics_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

# Parse args: single port (number) -> localhost:port; else ALL args are host:port list
if [[ -z "${1:-}" ]]; then
  NODES="localhost:8092"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="genomics-pipeline-rust"
ACTOR_TYPE="pipeline"
INSTANCE_ID="default"
NUM_READS=400000

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
echo ""

# Build array of host:port
read -ra NODE_LIST <<< "$NODES"
FIRST_NODE="${NODE_LIST[0]}"
HTTP_HOST_FIRST="${FIRST_NODE%%:*}"
HTTP_PORT_FIRST="${FIRST_NODE##*:}"

cleanup() {
  for node in "${NODE_LIST[@]}"; do
    h="${node%%:*}"
    p="${node##*:}"
    curl -s -X DELETE "http://${h}:${p}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

echo "================================================================"
echo "  Genomics Pipeline (Rust WASM)  nodes=${NODES}"
echo "================================================================"

# Check all nodes reachable
for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"
  p="${node##*:}"
  HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://${h}:${p}/" 2>/dev/null) || HTTP_CHECK="000"
  if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot reach node at ${h}:${p}. Start server(s) first (e.g. ./scripts/server.sh [port]).${NC}"
    exit 1
  fi
done

# ConnectNodes: entry node (first) connects to other nodes so leader can list workers and split work
# gRPC port = HTTP port - 1 (convention: server 8091 -> HTTP 8092)
if [ ${#NODE_LIST[@]} -gt 1 ]; then
  echo "Step 0b: ConnectNodes (entry node connects to peers for leader-worker)"
  PEER_GRPC_ADDRESSES=()
  for i in $(seq 1 $((${#NODE_LIST[@]} - 1))); do
    peer="${NODE_LIST[$i]}"
    ph="${peer%%:*}"
    pp="${peer##*:}"
    GRPC_PORT=$((pp - 1))
    PEER_GRPC_ADDRESSES+=("${ph}:${GRPC_PORT}")
  done
  CONNECT_BODY="{\"node_addresses\": [$(printf '"%s",' "${PEER_GRPC_ADDRESSES[@]}" | sed 's/,$//')]}"
  CONNECT_RESP=$(curl -s -w "\n%{http_code}" -X POST "http://${HTTP_HOST_FIRST}:${HTTP_PORT_FIRST}/api/v1/nodes:connect" \
    -H "Content-Type: application/json" -d "$CONNECT_BODY" 2>&1)
  CONNECT_CODE=$(echo "$CONNECT_RESP" | tail -n1)
  if [ "$CONNECT_CODE" != "200" ]; then
    echo -e "  ${YELLOW}ConnectNodes returned $CONNECT_CODE (continuing; leader-worker may not see peers)${NC}"
  else
    echo -e "  ${GREEN}Connected entry node to ${#PEER_GRPC_ADDRESSES[@]} peer(s)${NC}"
  fi
  sleep 1
fi

# Deploy to all nodes
for node in "${NODE_LIST[@]}"; do
  h="${node%%:*}"
  p="${node##*:}"
  echo "Step 1: Deploy to ${h}:${p}"
  curl -s -X DELETE "http://${h}:${p}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  sleep 1
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "http://${h}:${p}/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=genomics-pipeline-rust" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$CONFIG_FILE" 2>&1)
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" != "200" ] || ! echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "  ${RED}Deploy failed: $RESPONSE${NC}"; exit 1
  fi
  echo -e "  ${GREEN}Deployed to ${h}:${p}${NC}"
done
sleep 2

send_op() {
  local host="$1" port="$2" payload="$3" timeout="${4:-60}"
  curl -s --max-time "$timeout" -X POST "http://${host}:${port}/api/v1/actors/$APP_ID/${ACTOR_TYPE}:${INSTANCE_ID}?timeout=$timeout" \
    -H "Content-Type: application/json" -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Step 2: Send ONE run to the entry node. This example is single-actor: the pipeline runs entirely on the entry node (no leader/worker distribution).
RUN_PAYLOAD=$(cat <<EOF
{"op":"run","sample_id":"SAMPLE-2025-001","num_reads":$NUM_READS,"reference_genome":"hg38","min_quality_score":20}
EOF
)
echo "Step 2: Run pipeline (single actor on entry node ${FIRST_NODE}; no distribution to other nodes)"
RUN_START=$(date +%s%N)
RUN_RESP=$(send_op "$HTTP_HOST_FIRST" "$HTTP_PORT_FIRST" "$RUN_PAYLOAD" 90)
RUN_END=$(date +%s%N)
RUN_WALL_MS=$(( (RUN_END - RUN_START) / 1000000 ))

if ! echo "$RUN_RESP" | grep -q '"status":"completed"'; then
  echo -e "  ${RED}Run failed: $RUN_RESP${NC}"
  exit 1
fi
echo -e "  ${GREEN}Pipeline completed (wall ${RUN_WALL_MS} ms)${NC}"

# Step 2b: Per-node info (actors on each node) from entry node's list
if [ ${#NODE_LIST[@]} -gt 0 ]; then
  echo "Step 2b: Nodes (from entry node list)"
  NODES_JSON=$(curl -s --max-time 10 "http://${HTTP_HOST_FIRST}:${HTTP_PORT_FIRST}/api/v1/nodes?pageSize=100" 2>/dev/null || echo "{}")
  echo "$NODES_JSON" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    nodes = d.get('nodes', d.get('nodeRegistrations', []))
    if nodes:
        print('  node_id                    | actor_count (if present)')
        print('  --------------------------+--------------------------')
        for n in nodes:
            nid = n.get('nodeId') or n.get('node_id') or '?'
            ac = n.get('actorCount') or n.get('actor_count')
            ac_str = str(ac) if ac is not None else '—'
            print('  {:26} | {}'.format(nid[:26], ac_str))
    else:
        print('  (no nodes list in response)')
except Exception as e:
    print('  (parse error:', e)
" 2>/dev/null || true
  echo ""
fi

# Step 3: Query metrics from entry node
METRICS=$(send_op "$HTTP_HOST_FIRST" "$HTTP_PORT_FIRST" '{"op":"metrics"}' 10)
if ! echo "$METRICS" | grep -q "compute_time_ms"; then
  echo -e "  ${RED}Metrics query failed: $METRICS${NC}"
  exit 1
fi
NODE_COUNT=${#NODE_LIST[@]}
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARKS (single-actor pipeline on entry node; nodes deployed: ${NODE_COUNT})"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "$METRICS" | python3 -c "
import sys, json
try:
    d = json.load(sys.stdin)
    payload = d.get('payload', d)
    if isinstance(payload, str):
        payload = json.loads(payload) if payload else {}
    comp = payload.get('compute_time_ms', 0)
    coord = payload.get('coordinate_time_ms', 0)
    total = payload.get('total_time_ms', comp + coord)
    ratio = payload.get('granularity_ratio', (comp / coord) if coord else 0)
    eff = payload.get('efficiency', (comp / total * 100) if total else 0)
    node_count = $NODE_COUNT
    wall_ms = $RUN_WALL_MS
    num_reads = payload.get('data_size_reads', $NUM_READS)
    single = payload.get('single_actor', True)
    print()
    print('  Single-actor:   yes (pipeline runs on entry node only; no workers on other nodes)')
    print('  Data size:      {} reads (input)'.format(num_reads))
    print('  Nodes deployed: {} (only entry node runs this pipeline)'.format(node_count))
    print('  Wall (client): {} ms'.format(wall_ms))
    print('  Compute time:  {} ms'.format(comp))
    print('  Coord time:   {} ms'.format(coord))
    print('  Total:        {} ms'.format(total))
    print('  Efficiency:   {:.1f}%'.format(eff * 100 if eff <= 1 else eff))
    print('  Granularity:  {:.2f}x'.format(ratio))
    print()
except Exception as e:
    print('  (parse error:', e, ')', file=sys.stderr)
    sys.exit(1)
" 2>/dev/null || echo "  (could not parse metrics)"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Genomics Pipeline (WASM) test passed.${NC}"
exit 0
