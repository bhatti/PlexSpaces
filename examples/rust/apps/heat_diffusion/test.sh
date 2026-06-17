#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
_RUST_APPS_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# shellcheck source=/dev/null
source "${_RUST_APPS_ROOT}/test-common.sh"
WASM_FILE="$SCRIPT_DIR/heat_diffusion_actor.wasm"
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
  NODES=""
  for _port in "$@"; do
    NODES="${NODES:+$NODES }localhost:$_port"
  done
else
  NODES="$*"
  NODES="${NODES//,/ }"
fi

APP_ID="heat-diffusion-rust"
APP_NAME="heat-diffusion-rust"
LEADER_ACTOR="leader"
WIDTH=24000
NUM_REGIONS=16
MAX_ITERATIONS=30
TOLERANCE=0.0005

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
  local joined=""
  local sep=""
  for entry in "${NODE_LIST[@]}"; do
    joined="${joined}${sep}\"${entry}\""
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

TEMP_CONFIG="$(mktemp -t heat-diffusion-app-config)"
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
    deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${host}:${port}/api/v1/applications/deploy" \
      ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
      -F "application_id=$APP_ID" \
      -F "name=$APP_NAME" \
      -F "version=1.0.0" \
      -F "wasm_file=@$WASM_FILE;type=application/wasm" \
      -F "config=@$TEMP_CONFIG" 2>&1) || true
    http_code=$(echo "$deploy_output" | tail -n1)
    response=$(echo "$deploy_output" | sed '$d')
    if [ "$http_code" = "200" ] && echo "$response" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
      _deployed=1
      break
    fi
    echo "  Deploy attempt $_attempt to ${host}:${port} failed, retrying in 3s..."
    sleep 3
  done
  if [ "$_deployed" -eq 0 ]; then
    echo -e "${RED}Deploy to ${host}:${port} failed: $response${NC}"
    exit 1
  fi
done
_deployed=0
for _attempt in 1 2 3; do
  deploy_output=$(curl -s --connect-timeout 10 --max-time 180 -w "\n%{http_code}" -X POST "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=$APP_ID" \
    -F "name=$APP_NAME" \
    -F "version=1.0.0" \
    -F "wasm_file=@$WASM_FILE;type=application/wasm" \
    -F "config=@$TEMP_CONFIG" 2>&1) || true
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
  echo -e "${RED}Deploy failed: $response${NC}"
  exit 1
fi
echo -e "  ${GREEN}Deployed${NC}"
echo "Step 1b: Wait for cluster node discovery (async SWIM reconcile)"
wait_for_registry_membership "${#NODE_LIST[@]}" 5 2
echo -e "  ${GREEN}All ${#NODE_LIST[@]} nodes discovered${NC}"
rm -f "$TEMP_CONFIG"
TEMP_CONFIG=""

run_payload=$(cat <<EOF
{"op":"run","width":$WIDTH,"num_regions":$NUM_REGIONS,"max_iterations":$MAX_ITERATIONS,"tolerance":$TOLERANCE}
EOF
)

echo "Step 2: Trigger leader on ${ENTRY_NODE}"
run_attempt=1
run_attempt_limit=10
run_start=$(date +%s%N)
while true; do
  run_response=$(curl -s --max-time 180 -X POST \
    "http://${ENTRY_HOST}:${ENTRY_PORT}/api/v1/actors/$APP_ID/$LEADER_ACTOR/ask?timeout=180" \
    -H "Content-Type: application/json" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
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
export HEAT_WALL_MS="$wall_ms"
export HEAT_EXPECTED_NODES="${#NODE_LIST[@]}"
RUN_RESPONSE="$run_response" python3 - <<'PY' || {
import json
import os

raw = json.loads(os.environ["RUN_RESPONSE"])
payload = raw.get("payload", raw)
if isinstance(payload, str):
    payload = json.loads(payload)

compute_ms = int(payload.get("compute_time_ms", 0))
coord_ms = int(payload.get("coordination_time_ms", 0))
wall_ms = int(os.environ.get("HEAT_WALL_MS", "0"))
total = compute_ms + coord_ms
compute_pct = (compute_ms * 100.0 / total) if total else 0.0
coord_pct = (coord_ms * 100.0 / total) if total else 0.0
granularity = float(payload.get("granularity_ratio", 0.0))
avg_worker_latency = float(payload.get("avg_worker_latency_ms", 0.0))
max_worker_latency = float(payload.get("max_worker_latency_ms", 0.0))
message_count = int(payload.get("message_count", 0))
tuple_ops = int(payload.get("tuple_operation_count", 0))
actor_count = int(payload.get("actor_count", 0))
node_count = int(payload.get("node_count", 0))
worker_node_count = int(payload.get("worker_node_count", 0))
error_count = int(payload.get("error_count", 0))
nodes = payload.get("nodes", {})
node_addresses = payload.get("node_addresses", {})
roles = payload.get("roles", {})
remote_nodes_with_work = payload.get("remote_nodes_with_work", [])
expected_nodes = int(os.environ.get("HEAT_EXPECTED_NODES", "1"))

print("  Heat Diffusion (Rust WASM)")
print("  Data size")
print(
    "  width={width} regions={regions} iterations={iterations} total_points={points}".format(
        width=payload.get("width", 0),
        regions=payload.get("num_regions", 0),
        iterations=payload.get("final_iteration", 0),
        points=payload.get("total_points", 0),
    )
)
print("  Topology")
print(
    "  node_count={node_count} worker_node_count={worker_node_count} actor_count={actor_count} leader_node_id={leader} scatter_gather_rounds={rounds}".format(
        node_count=node_count,
        worker_node_count=worker_node_count,
        actor_count=actor_count,
        leader=payload.get("leader_node_id", ""),
        rounds=payload.get("scatter_gather_rounds", 0),
    )
)
print("  Timing")
print(f"  wall_time_ms={wall_ms}")
print(f"  compute_time_ms={compute_ms} ({compute_pct:.1f}%)")
print(f"  coordination_time_ms={coord_ms} ({coord_pct:.1f}%)")
print(f"  granularity={granularity:.2f}x")
print(f"  avg_worker_latency_ms={avg_worker_latency:.2f}")
print(f"  max_worker_latency_ms={max_worker_latency:.2f}")
print("  Throughput")
print(f"  message_count={message_count}")
print(f"  tuple_operation_count={tuple_ops}")
print("  Result")
print(f"  converged={payload.get('converged', False)}")
print(f"  max_diff={payload.get('max_diff', 0)}")
print(f"  errors={error_count}")
print(f"  remote_nodes_with_work={','.join(remote_nodes_with_work) if remote_nodes_with_work else 'none'}")
print(f"  actor_distribution_skew={payload.get('actor_distribution_skew', 0)}")
print("  Per-role")
for role_name in sorted(roles):
    role = roles[role_name]
    print(
        "  {role}: actors={actors} messages={messages} tuple_ops={tuple_ops} "
        "compute_ms={compute_ms} coord_ms={coord_ms} avg_latency_ms={avg_latency:.2f} "
        "max_latency_ms={max_latency} errors={errors}".format(
            role=role_name,
            actors=role.get("actors", 0),
            messages=role.get("messages", 0),
            tuple_ops=role.get("tuple_operations", 0),
            compute_ms=role.get("compute_time_ms", 0),
            coord_ms=role.get("coordination_time_ms", 0),
            avg_latency=float(role.get("avg_latency_ms", 0.0)),
            max_latency=role.get("max_latency_ms", 0),
            errors=role.get("errors", 0),
        )
    )
print("  Per-node")
for node_id in sorted(nodes):
    node = nodes[node_id]
    node_address = node_addresses.get(node_id, "unknown")
    print(
        "  {node_id} ({node_address}): actors={actors} leader_actors={leader_actors} worker_actors={worker_actors} "
        "messages={messages} leader_messages={leader_messages} worker_messages={worker_messages} tuple_ops={tuple_ops} "
        "compute_ms={compute_ms} coord_ms={coord_ms} avg_latency_ms={avg_latency:.2f} "
        "max_latency_ms={max_latency} errors={errors}".format(
            node_id=node_id,
            node_address=node_address,
            actors=node.get("actors", 0),
            leader_actors=node.get("leader_actors", 0),
            worker_actors=node.get("worker_actors", 0),
            messages=node.get("messages", 0),
            leader_messages=node.get("leader_messages", 0),
            worker_messages=node.get("worker_messages", 0),
            tuple_ops=node.get("tuple_operations", 0),
            compute_ms=node.get("compute_time_ms", 0),
            coord_ms=node.get("coordination_time_ms", 0),
            avg_latency=float(node.get("avg_latency_ms", 0.0)),
            max_latency=node.get("max_latency_ms", 0),
            errors=node.get("errors", 0),
        )
    )

if node_count < 1 or actor_count < 2 or message_count < 1:
    raise SystemExit("metrics payload missing expected topology/message counts")
if expected_nodes > 1 and node_count < expected_nodes:
    raise SystemExit(
        f"expected workload to use {expected_nodes} nodes but metrics only reported {node_count}"
    )
if expected_nodes > 1 and worker_node_count < expected_nodes - 1:
    raise SystemExit(
        f"expected workers to use {expected_nodes - 1} remote nodes but metrics only reported {worker_node_count}"
    )
if expected_nodes > 1 and not remote_nodes_with_work:
    raise SystemExit("expected at least one remote node to process worker messages")
PY
  echo -e "${RED}Metrics validation failed${NC}"
  exit 1
}

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Heat diffusion test passed.${NC}"
