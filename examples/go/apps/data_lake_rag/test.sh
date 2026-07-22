#!/usr/bin/env bash
# gRPC and HTTP share a single port — NODES lists single port per node.
# "Placement produced no target nodes" means list_nodes was empty when the leader ran
# (registry / cluster / connectivity).
#
# Debugging:
#   SKIP_TEST_UNDEPLOY=1 ./test.sh   — on EXIT, do not DELETE the app (trap). Otherwise a failed
#   run still undeploys and you will see leader "shutdown lifecycle" in the gRPC node's log.
#   Asks go only to ENTRY_NODE (default first port, e.g. 8091); the other node stays quiet
#   unless you curl it or traffic routes there.
#   DATA_LAKE_RAG_POST_DEPLOY_SECS / DATA_LAKE_RAG_PRE_LIST_NODES_SECS — extra sleep after deploy
#   and immediately before listing nodes (helps _unknown_ peer ids and registry settle).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/data_lake_rag_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
export CARGO_PROFILE="${CARGO_PROFILE:-debug}"


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

if [[ -f "$HOME/venv/bin/activate" ]]; then
  # shellcheck disable=SC1090
  source "$HOME/venv/bin/activate"
fi

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  # All-numeric args are bare ports: "8091 8094" → "localhost:8091 localhost:8094"
  NODES=""
  for _port in "$@"; do
    NODES="${NODES:+$NODES }localhost:$_port"
  done
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

echo "Step 0: Build WASM"
"$SCRIPT_DIR/build.sh"
echo ""

if [ ! -f "$WASM_FILE" ]; then
  echo -e "${RED}Build did not produce $WASM_FILE${NC}"
  exit 1
fi

for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  http_code="000"
  for _i in 1 2 3; do
    http_code=$(curl -s -o /dev/null -w "%{http_code}" "http://${host}:${port}/" 2>/dev/null) || http_code="000"
    [ "$http_code" != "000" ] && break
    sleep 2
  done
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot reach node at ${host}:${port}${NC}"
    exit 1
  fi
done

TEMP_CONFIG="$(mktemp -t data-lake-rag-app-config)"
    APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
    zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
render_config "$TEMP_CONFIG"

echo "Step 1: Undeploy existing app from all nodes"
"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2

echo "Step 1: Deploy to all nodes (non-entry nodes first, entry node last)"
# Deploy to non-entry nodes first so behaviors are registered before
# the entry node starts the leader (which triggers CreateShardGroup immediately).
for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then
    continue
  fi
  host="${node%%:*}"
  port="${node##*:}"
  echo "  Deploying to ${host}:${port} (worker node)..."
  _deployed=0
  for _attempt in 1 2 3; do
    deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${host}:${port}/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "app_file=@$APP_ZIP" 2>&1) || true
    http_code=$(echo "$deploy_output" | tail -n1)
    response=$(echo "$deploy_output" | sed '$d')
    if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
      _deployed=1
      break
    fi
    echo "    Deploy attempt $_attempt failed, retrying in 3s..."
    sleep 3
  done
  if [ "$_deployed" -eq 0 ]; then
    echo -e "${RED}Deploy to ${host}:${port} failed: $response${NC}"
    exit 1
  fi
  echo -e "  ${GREEN}Deployed to ${host}:${port}${NC}"
done
# Deploy to entry node last (starts leader which triggers CreateShardGroup)
echo "  Deploying to ${ENTRY_HOST}:${ENTRY_PORT} (entry node)..."
_deployed=0
for _attempt in 1 2 3; do
  APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
  zip -j "$APP_ZIP" "$WASM_FILE" "$TEMP_CONFIG" >/dev/null
  deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  http_code=$(echo "$deploy_output" | tail -n1)
  response=$(echo "$deploy_output" | sed '$d')
  if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy to ${ENTRY_HOST}:${ENTRY_PORT} failed: $response${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed to ${ENTRY_HOST}:${ENTRY_PORT}${NC}"
# SWIM + async seed ping need time so each node's registry lists peers (not only self).
# Override with DATA_LAKE_RAG_POST_DEPLOY_SECS if your cluster is slower.
sleep "${DATA_LAKE_RAG_POST_DEPLOY_SECS:-5}"
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""

# Extra settle immediately before listing nodes so peer IDs replace _unknown_ in HTTP registry output.
# This is in addition to POST_DEPLOY_SECS. Override with DATA_LAKE_RAG_PRE_LIST_NODES_SECS (0 to skip).
echo "Step 1a: Pre-list settle (${DATA_LAKE_RAG_PRE_LIST_NODES_SECS:-5}s before GET /api/v1/nodes)"
sleep "${DATA_LAKE_RAG_PRE_LIST_NODES_SECS:-5}"

echo "Step 1b: ListConnectedNodes (GET /api/v1/nodes) per node — same source as gRPC ListConnectedNodes"
echo "  (post_deploy=${DATA_LAKE_RAG_POST_DEPLOY_SECS:-5}s + pre_list=${DATA_LAKE_RAG_PRE_LIST_NODES_SECS:-5}s; _unknown_ clears after successful peer ping)"
for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  echo "  Registered nodes from http://${host}:${port}/api/v1/nodes"
  curl -s --connect-timeout 5 --max-time 30 ${AUTH_HEADER:+-H "$AUTH_HEADER"} "http://${host}:${port}/api/v1/nodes?page_size=100" | sed 's/^/    /' || echo "    (request failed)"
done
echo ""

echo "Step 1c: Server logs (read on the terminals where each node process runs)"
echo "  Set RUST_LOG=info or RUST_LOG=debug before starting nodes. Watch for gossip/SWIM peers,"
echo "  node id resolution, and [WASM] lines from data_lake_rag (host.Info)."
echo "  If Step 2 fails placement or metrics show workers only on the leader, logs show whether"
echo "  remote gRPC peers and shard placement matched the registry."
echo ""

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
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "$run_payload" 2>/dev/null || echo '{"error":"timeout"}')

  if [[ "$run_response" != *"Placement produced no target nodes"* ]]; then
    break
  fi

  if (( run_attempt >= run_attempt_limit )); then
    break
  fi

  # Ask/mailbox succeeded; failure is inside payload (CreateShardGroup + from_registry needs list_nodes non-empty).
  echo -e "  ${YELLOW}Placement still empty (node registry has no members for this cluster). Retry ${run_attempt}/${run_attempt_limit} — check gRPC peers, app seed_nodes, release cluster_seed_nodes, mTLS.${NC}"
  run_attempt=$((run_attempt + 1))
  sleep 2
done
run_end=$(date +%s%N)
wall_ms=$(( (run_end - run_start) / 1000000 ))

if ! echo "$run_response" | grep -q '"status":"ok"'; then
  echo -e "${RED}Leader run failed (HTTP ask may still be 200; payload missing status=ok):${NC}"
  echo "$run_response"
  if echo "$run_response" | grep -q "Placement produced no target nodes"; then
    echo -e "${YELLOW}Hint: from_registry placement uses an empty node list — fix multinode membership before re-running.${NC}"
  fi
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
worker_remote_shard_nodes = payload.get("worker_remote_shard_nodes") or []
expected_nodes = int(os.environ.get("DATA_LAKE_RAG_EXPECTED_NODES", "1"))
needed_remote_hosts = max(0, expected_nodes - 1)

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
    "  node_count={node_count} worker_node_count={worker_nodes} worker_remote_shard_nodes={hosts} actor_count={actor_count} leader_node_id={leader} retrieval_rounds={rounds}".format(
        node_count=node_count,
        worker_nodes=worker_node_count,
        hosts=",".join(worker_remote_shard_nodes) if worker_remote_shard_nodes else "none",
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
if expected_nodes > 1 and len(worker_remote_shard_nodes) < needed_remote_hosts:
    raise SystemExit(
        f"expected worker shard placement on at least {needed_remote_hosts} remote host(s), got {len(worker_remote_shard_nodes)} ({worker_remote_shard_nodes!r}). "
        "Check from_registry / cluster membership (see Step 1c logs)."
    )
if worker_node_count < needed_remote_hosts:
    raise SystemExit(
        f"expected worker_node_count>={needed_remote_hosts} (from placement + status), got {worker_node_count}"
    )
if error_count == 0 and expected_nodes > 1 and not remote_nodes_with_work:
    raise SystemExit(
        "expected remote_nodes_with_work when error_count is zero (no successful shard payload from remote workers?)"
    )
if error_count != 0:
    raise SystemExit(
        f"expected zero shard/handler errors but saw {error_count}. "
        "Check node stderr: application-metrics-add / decode ApplicationMetrics, or WASM 'worker search metrics update'."
    )
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
