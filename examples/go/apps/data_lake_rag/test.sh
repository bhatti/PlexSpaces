#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/data_lake_rag_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8092 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES="localhost:$1"
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="data-lake-rag-go"
APP_NAME="data-lake-rag-go"
LEADER_ACTOR="leader"
WORKER_COUNT=8
QUERY_COUNT=12
CHUNKS_PER_QUERY=48
CHUNK_SIZE_BYTES=2048
EMBEDDING_DIM=384
TOP_K=5

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"
TEMP_CONFIG=""

cleanup() {
  if [[ -n "${TEMP_CONFIG:-}" && -f "${TEMP_CONFIG:-}" ]]; then
    rm -f "$TEMP_CONFIG"
  fi
  for node in "${NODE_LIST[@]}"; do
    local host="${node%%:*}"
    local port="${node##*:}"
    curl -s -X DELETE "http://${host}:${port}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  done
}
trap cleanup EXIT

grpc_seed_nodes() {
  local seed_list=()
  for node in "${NODE_LIST[@]}"; do
    local host="${node%%:*}"
    local port="${node##*:}"
    seed_list+=("\"${host}:$((port - 1))\"")
  done
  local joined=""
  local sep=""
  for entry in "${seed_list[@]}"; do
    joined="${joined}${sep}${entry}"
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

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${host}:${port}/" 2>/dev/null) || http_code="000"
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot reach node at ${host}:${port}${NC}"
    exit 1
  fi
done

TEMP_CONFIG="$(mktemp -t data-lake-rag-app-config)"
render_config "$TEMP_CONFIG"

for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  echo "Step 1: Deploy to ${host}:${port}"
  curl -s -X DELETE "http://${host}:${port}/api/v1/applications/$APP_ID" >/dev/null 2>&1 || true
  sleep 1
  deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${host}:${port}/api/v1/applications/deploy" \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG" 2>&1)
  http_code=$(echo "$deploy_output" | tail -n1)
  response=$(echo "$deploy_output" | sed '$d')
  if [ "$http_code" != "200" ] || ! echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    echo -e "${RED}Deploy failed on ${host}:${port}: $response${NC}"
    exit 1
  fi
  echo -e "  ${GREEN}Deployed${NC}"
done
sleep 2
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""

run_payload=$(cat <<EOF
{"op":"run","worker_count":$WORKER_COUNT,"query_count":$QUERY_COUNT,"chunks_per_query":$CHUNKS_PER_QUERY,"chunk_size_bytes":$CHUNK_SIZE_BYTES,"embedding_dim":$EMBEDDING_DIM,"top_k":$TOP_K}
EOF
)

echo "Step 2: Trigger leader on ${ENTRY_NODE}"
run_attempt=1
run_attempt_limit=10
run_start=$(date +%s%N)
while true; do
  run_response=$(curl -s --max-time 240 -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$LEADER_ACTOR/ask?timeout=240" \
    -H "Content-Type: application/json" \
    -d "$run_payload" 2>/dev/null || echo '{"error":"timeout"}')

  if [[ "$run_response" != *"Placement produced no target nodes"* ]]; then
    break
  fi

  if (( run_attempt >= run_attempt_limit )); then
    break
  fi

  echo -e "  ${YELLOW}Seed nodes not reconciled yet, retrying leader run (${run_attempt}/${run_attempt_limit})...${NC}"
  run_attempt=$((run_attempt + 1))
  sleep 2
done
run_end=$(date +%s%N)
wall_ms=$(( (run_end - run_start) / 1000000 ))

if ! echo "$run_response" | grep -q '"status":"ok"'; then
  echo -e "${RED}Leader run failed: $run_response${NC}"
  exit 1
fi

echo "Step 3: Metrics"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
export DATA_LAKE_RAG_WALL_MS="$wall_ms"
export DATA_LAKE_RAG_EXPECTED_NODES="${#NODE_LIST[@]}"
RUN_RESPONSE="$run_response" python3 - <<'PY' || {
import json
import os

raw = json.loads(os.environ["RUN_RESPONSE"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)

compute_ms = int(payload.get("compute_time_ms", 0))
coord_ms = int(payload.get("coordination_time_ms", 0))
wall_ms = int(os.environ.get("DATA_LAKE_RAG_WALL_MS", "0"))
total = compute_ms + coord_ms
compute_pct = (compute_ms * 100.0 / total) if total else 0.0
coord_pct = (coord_ms * 100.0 / total) if total else 0.0
node_count = int(payload.get("node_count", 0))
worker_node_count = int(payload.get("worker_node_count", 0))
actor_count = int(payload.get("actor_count", 0))
message_count = int(payload.get("message_count", 0))
chunk_ops = int(payload.get("chunk_operation_count", 0))
embed_ops = int(payload.get("embedding_operation_count", 0))
retrieval_ops = int(payload.get("retrieval_operation_count", 0))
bytes_ingested = int(payload.get("bytes_ingested", 0))
error_count = int(payload.get("error_count", 0))
nodes = payload.get("nodes", {})
node_addresses = payload.get("node_addresses", {})
roles = payload.get("roles", {})
remote_nodes_with_work = payload.get("remote_nodes_with_work", [])
expected_nodes = int(os.environ.get("DATA_LAKE_RAG_EXPECTED_NODES", "1"))

print("  Data Lake RAG (Go WASM)")
print("  Data size")
print(
    "  query_count={queries} worker_count={workers} chunks_per_query={chunks} chunk_size_bytes={chunk_size} embedding_dim={embedding_dim} top_k={top_k}".format(
        queries=payload.get("query_count", 0),
        workers=payload.get("worker_count", 0),
        chunks=payload.get("chunks_per_query", 0),
        chunk_size=payload.get("chunk_size_bytes", 0),
        embedding_dim=payload.get("embedding_dim", 0),
        top_k=payload.get("top_k", 0),
    )
)
print("  Topology")
print(
    "  node_count={node_count} worker_node_count={worker_nodes} actor_count={actor_count} leader_node_id={leader} retrieval_rounds={rounds}".format(
        node_count=node_count,
        worker_nodes=worker_node_count,
        actor_count=actor_count,
        leader=payload.get("leader_node_id", ""),
        rounds=payload.get("retrieval_rounds", 0),
    )
)
print("  Timing")
print(f"  wall_time_ms={wall_ms}")
print(f"  compute_time_ms={compute_ms} ({compute_pct:.1f}%)")
print(f"  coordination_time_ms={coord_ms} ({coord_pct:.1f}%)")
print(f"  granularity={float(payload.get('granularity_ratio', 0.0)):.2f}x")
print(f"  avg_worker_latency_ms={float(payload.get('avg_worker_latency_ms', 0.0)):.2f}")
print(f"  max_worker_latency_ms={float(payload.get('max_worker_latency_ms', 0.0)):.2f}")
print("  Throughput")
print(f"  message_count={message_count}")
print(f"  chunk_operation_count={chunk_ops}")
print(f"  embedding_operation_count={embed_ops}")
print(f"  retrieval_operation_count={retrieval_ops}")
print(f"  bytes_ingested={bytes_ingested}")
print("  Result")
print(f"  errors={error_count}")
print(
    "  remote_nodes_with_work={nodes}".format(
        nodes="none" if not remote_nodes_with_work else ",".join(remote_nodes_with_work)
    )
)
print(f"  actor_distribution_skew={int(payload.get('actor_distribution_skew', 0))}")
print("  Per-role")
for role in sorted(roles):
    info = roles[role]
    print(
        "  {role}: actors={actors} messages={messages} chunk_ops={chunk_ops} embedding_ops={embedding_ops} retrieval_ops={retrieval_ops} compute_ms={compute_ms} coord_ms={coord_ms} avg_latency_ms={avg:.2f} max_latency_ms={max_latency} errors={errors}".format(
            role=role,
            actors=info.get("actors", 0),
            messages=info.get("messages", 0),
            chunk_ops=info.get("chunk_operations", 0),
            embedding_ops=info.get("embedding_operations", 0),
            retrieval_ops=info.get("retrieval_operations", 0),
            compute_ms=info.get("compute_time_ms", 0),
            coord_ms=info.get("coordination_time_ms", 0),
            avg=float(info.get("avg_latency_ms", 0.0)),
            max_latency=info.get("max_latency_ms", 0),
            errors=info.get("errors", 0),
        )
    )
print("  Per-node")
for node_id in sorted(nodes):
    info = nodes[node_id]
    address = node_addresses.get(node_id, "unknown")
    print(
        "  {node_id} ({address}): actors={actors} leader_actors={leader_actors} worker_actors={worker_actors} messages={messages} leader_messages={leader_messages} worker_messages={worker_messages} chunk_ops={chunk_ops} embedding_ops={embedding_ops} retrieval_ops={retrieval_ops} bytes_ingested={bytes_ingested} compute_ms={compute_ms} coord_ms={coord_ms} avg_latency_ms={avg:.2f} max_latency_ms={max_latency} errors={errors}".format(
            node_id=node_id,
            address=address,
            actors=info.get("actors", 0),
            leader_actors=info.get("leader_actors", 0),
            worker_actors=info.get("worker_actors", 0),
            messages=info.get("messages", 0),
            leader_messages=info.get("leader_messages", 0),
            worker_messages=info.get("worker_messages", 0),
            chunk_ops=info.get("chunk_operations", 0),
            embedding_ops=info.get("embedding_operations", 0),
            retrieval_ops=info.get("retrieval_operations", 0),
            bytes_ingested=info.get("bytes_ingested", 0),
            compute_ms=info.get("compute_time_ms", 0),
            coord_ms=info.get("coordination_time_ms", 0),
            avg=float(info.get("avg_latency_ms", 0.0)),
            max_latency=info.get("max_latency_ms", 0),
            errors=info.get("errors", 0),
        )
    )

if node_count < expected_nodes:
    raise SystemExit(f"expected at least {expected_nodes} nodes in metrics but saw {node_count}")
if worker_node_count < max(0, expected_nodes - 1):
    raise SystemExit(
        f"expected workers to use {max(0, expected_nodes - 1)} remote nodes but metrics only reported {worker_node_count}"
    )
if not remote_nodes_with_work and expected_nodes > 1:
    raise SystemExit("expected remote worker activity but metrics reported none")
if error_count != 0:
    raise SystemExit(f"expected zero errors but saw {error_count}")
if chunk_ops <= 0:
    raise SystemExit("expected positive chunk operation count")
if embed_ops <= 0:
    raise SystemExit("expected positive embedding operation count")
if retrieval_ops <= 0:
    raise SystemExit("expected positive retrieval operation count")
if bytes_ingested <= 0:
    raise SystemExit("expected positive bytes_ingested count")
PY
  echo -e "${RED}Metrics validation failed${NC}"
  exit 1
}
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Data lake RAG test passed.${NC}"
