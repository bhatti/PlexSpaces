#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_PYTHON_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_PYTHON_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/parameter_server_actor.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"

if [[ -z "${1:-}" ]]; then
  NODES="localhost:8091 localhost:8094"
elif [[ "$1" =~ ^[0-9]+$ ]]; then
  NODES=""
  for _port in "$@"; do
    NODES="${NODES:+$NODES }localhost:$_port"
  done
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

APP_ID="python-parameter-server"
APP_NAME="python-parameter-server"
LEADER_ACTOR="leader"
TRAIN_ITERATIONS=10
INPUT_DIM=100
HIDDEN_DIM=64
BATCH_SIZE=256
TEMP_CONFIG=""

REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

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
  echo "  Token: ${PLEXSPACES_TEST_TOKEN:0:20}..."
fi
export AUTH_HEADER=""
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi
echo "AUTH_HEADER set: $([ -n "$AUTH_HEADER" ] && echo YES || echo NO)"

read -ra NODE_LIST <<< "$NODES"
ENTRY_NODE="${NODE_LIST[0]}"
ENTRY_HOST="${ENTRY_NODE%%:*}"
ENTRY_PORT="${ENTRY_NODE##*:}"


grpc_seed_nodes() {
  local result=""
  for _n in "${NODE_LIST[@]}"; do
    local _h="${_n%%:*}"
    local _p="${_n##*:}"
    [ -n "$result" ] && result="${result}, "
    result="${result}\"http://${_h}:${_p}\""
  done
  printf '%s' "$result"
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
seed_replaced = False
for line in source.splitlines():
    if line.startswith("seed_nodes = "):
        lines.append(f"seed_nodes = [{seed_nodes}]")
        seed_replaced = True
    else:
        lines.append(line)
if not seed_replaced:
    lines.insert(3, f"seed_nodes = [{seed_nodes}]")
pathlib.Path(sys.argv[2]).write_text("\n".join(lines) + "\n")
PY
}

if [ ! -f "$WASM_FILE" ]; then
  "$SCRIPT_DIR/build.sh"
fi

for node in "${NODE_LIST[@]}"; do
  host="${node%%:*}"
  port="${node##*:}"
  http_code="000"
  for _i in 1 2 3; do
    if [ -n "$AUTH_HEADER" ]; then
      http_code=$(curl -s --max-time 5 -o /dev/null -w "%{http_code}" -H "$AUTH_HEADER" "http://${host}:${port}/" 2>/dev/null) || http_code="000"
    else
      http_code=$(curl -s --max-time 5 -o /dev/null -w "%{http_code}" "http://${host}:${port}/" 2>/dev/null) || http_code="000"
    fi
    [ "$http_code" != "000" ] && break
    sleep 2
  done
  if [ "$http_code" = "000" ]; then
    echo -e "${RED}Cannot connect to node at ${host}:${port}${NC}"
    exit 1
  fi
done

TEMP_CONFIG="$(mktemp -t python-parameter-server-app-config)"
render_config "$TEMP_CONFIG"

echo "Step 1: Undeploy from all nodes, then deploy to all nodes"
"$SCRIPT_DIR/undeploy.sh" $NODES
sleep 2
for node in "${NODE_LIST[@]}"; do
  if [ "$node" = "$ENTRY_NODE" ]; then
    continue
  fi
  host="${node%%:*}"
  port="${node##*:}"
  _deployed=0
  for _attempt in 1 2 3; do
    if [ -n "$AUTH_HEADER" ]; then
      response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${host}:${port}/api/v1/applications/deploy" \
        -H "$AUTH_HEADER" \
        -F "application_id=$APP_ID" \
        -F "name=$APP_NAME" \
        -F "version=1.0.0" \
        -F "wasm_file=@$WASM_FILE;type=application/wasm" \
        -F "config=@$TEMP_CONFIG" 2>&1) || true
    else
      response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${host}:${port}/api/v1/applications/deploy" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -F "application_id=$APP_ID" \
        -F "name=$APP_NAME" \
        -F "version=1.0.0" \
        -F "wasm_file=@$WASM_FILE;type=application/wasm" \
        -F "config=@$TEMP_CONFIG" 2>&1) || true
    fi
    http_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')
    if [ "$http_code" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
      _deployed=1
      break
    fi
    echo "  Deploy attempt $_attempt to ${host}:${port} failed (HTTP $http_code), retrying in 3s..."
    echo "  Response: $(echo "$body" | head -c 200)"
    sleep 3
  done
  if [ "$_deployed" -eq 0 ]; then
    echo -e "${RED}Deploy to ${host}:${port} failed: $body${NC}"
    exit 1
  fi
done
_deployed=0
for _attempt in 1 2 3; do
  if [ -n "$AUTH_HEADER" ]; then
    response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -H "$AUTH_HEADER" \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$TEMP_CONFIG" 2>&1) || true
  else
    response=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$TEMP_CONFIG" 2>&1) || true
  fi
  http_code=$(echo "$response" | tail -n1)
  body=$(echo "$response" | sed '$d')
  if [ "$http_code" = "200" ] && echo "$body" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  echo "  Deploy attempt $_attempt failed (HTTP $http_code), retrying in 3s..."
  echo "  Response: $(echo "$body" | head -c 200)"
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $body${NC}"
  exit 1
fi
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""
echo "Step 1b: Wait for cluster node discovery (async SWIM reconcile)"
wait_for_registry_membership "${#NODE_LIST[@]}" 5 2
echo -e "  ${GREEN:-}All ${#NODE_LIST[@]} nodes discovered${NC:-}"

echo "Step 2: Trigger leader on ${ENTRY_NODE}"
run_start=$(date +%s%N)
if [ -n "$AUTH_HEADER" ]; then
  RUN_RESPONSE=$(curl -s --max-time 120 -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$LEADER_ACTOR/ask?timeout=120" \
    -H "$AUTH_HEADER" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "{\"op\":\"train\",\"iterations\":$TRAIN_ITERATIONS}" 2>/dev/null || echo '{"error":"timeout"}')
else
  RUN_RESPONSE=$(curl -s --max-time 120 -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$LEADER_ACTOR/ask?timeout=120" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -d "{\"op\":\"train\",\"iterations\":$TRAIN_ITERATIONS}" 2>/dev/null || echo '{"error":"timeout"}')
fi
run_end=$(date +%s%N)
wall_ms=$(( (run_end - run_start) / 1000000 ))

if ! echo "$RUN_RESPONSE" | grep -q '"status":"ok"'; then
  echo -e "${RED}Training failed: $RUN_RESPONSE${NC}"
  exit 1
fi

echo "Step 3: Metrics"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
export PARAMETER_SERVER_WALL_MS="$wall_ms"
export PARAMETER_SERVER_EXPECTED_NODES="${#NODE_LIST[@]}"
export PARAMETER_SERVER_INPUT_DIM="$INPUT_DIM"
export PARAMETER_SERVER_HIDDEN_DIM="$HIDDEN_DIM"
export PARAMETER_SERVER_BATCH_SIZE="$BATCH_SIZE"
RUN_RESPONSE="$RUN_RESPONSE" python3 - <<'PY' || {
import json
import os
import sys

raw = json.loads(os.environ["RUN_RESPONSE"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)

compute_ms = int(payload.get("compute_time_ms", 0))
coord_ms = int(payload.get("coordination_time_ms", 0))
wall_ms = int(os.environ.get("PARAMETER_SERVER_WALL_MS", "0"))
total = compute_ms + coord_ms
compute_pct = (compute_ms * 100.0 / total) if total else 0.0
coord_pct = (coord_ms * 100.0 / total) if total else 0.0
nodes = payload.get("nodes", {})
node_addresses = payload.get("node_addresses", {})
roles = payload.get("roles", {})
remote_nodes_with_work = payload.get("remote_nodes_with_work", [])
expected_nodes = int(os.environ.get("PARAMETER_SERVER_EXPECTED_NODES", "1"))

print("  Parameter Server (Python WASM)")
print("  Data size")
print(
    "  param_count={params} worker_count={workers} iterations={iterations} batch_size={batch}".format(
        params=payload.get("param_count", 0),
        workers=payload.get("worker_count", 0),
        iterations=payload.get("iterations", 0),
        batch=payload.get("batch_size", 0),
    )
)
print("  Topology")
print(
    "  node_count={node_count} worker_node_count={worker_nodes} actor_count={actor_count} leader_node_id={leader} training_rounds={rounds}".format(
        node_count=payload.get("node_count", 0),
        worker_nodes=payload.get("worker_node_count", 0),
        actor_count=payload.get("actor_count", 0),
        leader=payload.get("leader_node_id", ""),
        rounds=payload.get("training_rounds", 0),
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
print(f"  message_count={int(payload.get('message_count', 0))}")
print(f"  gradient_operation_count={int(payload.get('gradient_operation_count', 0))}")
print(f"  samples_processed={int(payload.get('samples_processed', 0))}")
print(f"  weight_update_count={int(payload.get('weight_update_count', 0))}")
print("  Result")
print(f"  errors={int(payload.get('error_count', 0))}")
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
        "  {role}: actors={actors} messages={messages} gradient_ops={gradient_ops} samples={samples} compute_ms={compute_ms} coord_ms={coord_ms} avg_latency_ms={avg:.2f} max_latency_ms={max_latency} errors={errors}".format(
            role=role,
            actors=info.get("actors", 0),
            messages=info.get("messages", 0),
            gradient_ops=info.get("gradient_operations", 0),
            samples=info.get("samples_processed", 0),
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
    address = node_addresses.get(node_id, "")
    label = f"{node_id} ({address})" if address else node_id
    print(
        "  {node}: actors={actors} leader_actors={leader_actors} worker_actors={worker_actors} messages={messages} leader_messages={leader_messages} worker_messages={worker_messages} gradient_ops={gradient_ops} samples={samples} compute_ms={compute_ms} coord_ms={coord_ms} avg_latency_ms={avg:.2f} max_latency_ms={max_latency} errors={errors}".format(
            node=label,
            actors=info.get("actors", 0),
            leader_actors=info.get("leader_actors", 0),
            worker_actors=info.get("worker_actors", 0),
            messages=info.get("messages", 0),
            leader_messages=info.get("leader_messages", 0),
            worker_messages=info.get("worker_messages", 0),
            gradient_ops=info.get("gradient_operations", 0),
            samples=info.get("samples_processed", 0),
            compute_ms=info.get("compute_time_ms", 0),
            coord_ms=info.get("coordination_time_ms", 0),
            avg=float(info.get("avg_latency_ms", 0.0)),
            max_latency=info.get("max_latency_ms", 0),
            errors=info.get("errors", 0),
        )
    )
print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")

if int(payload.get("error_count", 0)) != 0:
    print("expected zero worker errors")
    sys.exit(1)
if expected_nodes > 1 and int(payload.get("worker_node_count", 0)) < 1:
    print(
        "expected workers to use {expected} remote nodes but metrics only reported {actual}".format(
            expected=expected_nodes - 1,
            actual=payload.get("worker_node_count", 0),
        )
    )
    sys.exit(1)
if expected_nodes > 1 and not remote_nodes_with_work:
    print("expected remote_nodes_with_work to include at least one remote node")
    sys.exit(1)
PY
  echo -e "${RED}Metrics validation failed${NC}"
  exit 1
}

echo -e "${GREEN}Python parameter server example passed.${NC}"
