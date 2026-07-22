#!/bin/bash
# Test Ray Parameter Server - Distributed ML Training (Python WASM)
#
# Usage: ./test.sh [HTTP_PORT]
#   HTTP_PORT: PlexSpaces HTTP gateway port (default: 8091)
#
# Prerequisites: Start PlexSpaces node first:
#   PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8091
# (HTTP gateway will be on 8091 + 1 = 8091)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WASM_FILE="$SCRIPT_DIR/ray_actors.wasm"
CONFIG_FILE="$SCRIPT_DIR/app-config.toml"
HTTP_PORT="${1:-8091}"

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

APP_ID="ray-ps"
NUM_WORKERS=2
TRAIN_ITERATIONS=10



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

echo "================================================================"
echo "  Ray Parameter Server - Distributed ML Training"
echo "  PlexSpaces Python WASM App"
echo "================================================================"
echo ""
echo "Configuration:"
echo "  HTTP Gateway:   localhost:$HTTP_PORT"
echo "  Workers:        $NUM_WORKERS"
echo "  Iterations:     $TRAIN_ITERATIONS"
echo "  Model:          100x64 (6,464 parameters)"
echo "  Data per worker: 2,000 samples (batch=256)"
echo ""

# Build if needed
if [ ! -f "$WASM_FILE" ]; then
    echo "Building WASM actor..."
    chmod +x "$SCRIPT_DIR/build.sh"
    "$SCRIPT_DIR/build.sh" || { echo -e "${RED}Build failed${NC}"; exit 1; }
    echo ""
fi

# Check node
echo "Step 1: Check node status"
echo "----------------------------------------------------------------"
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "$WASM_FILE" "$CONFIG_FILE" >/dev/null
HTTP_CHECK=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$HTTP_PORT/" 2>/dev/null) || HTTP_CHECK="000"
if [ "$HTTP_CHECK" = "000" ]; then
    echo -e "${RED}Cannot connect to node at localhost:$HTTP_PORT${NC}"
    echo "Start node first: PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:$((HTTP_PORT - 1))"
    exit 1
fi
echo -e "${GREEN}Node is running${NC}"
echo ""


# Deploy
echo "Step 2: Deploy parameter server + $NUM_WORKERS workers"
echo "----------------------------------------------------------------"

"$SCRIPT_DIR/undeploy.sh" "$HTTP_PORT"
sleep 1

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
  echo "  Deploy attempt $_attempt failed, retrying in 3s..."
  sleep 3
done
if [ "$_deployed" -eq 0 ]; then
  echo -e "${RED}Deploy failed: $RESPONSE${NC}"
  exit 1
fi

if echo "$RESPONSE" | grep -qi '"success":\s*true'; then
    echo -e "${GREEN}Deployed $APP_ID${NC}"
    echo "  - Parameter server: parameter-server"
    echo "  - Data workers: data-worker-0..data-worker-$((NUM_WORKERS - 1))"
else
    echo -e "${RED}Deploy failed: $RESPONSE${NC}"
    exit 1
fi
echo ""
sleep 2

# Helper to send POST and get response
# Usage: send_op <actor> <payload> [timeout_secs]
send_op() {
    local actor="$1"
    local payload="$2"
    local timeout="${3:-60}"
    curl -s --max-time "$timeout" -X POST "http://localhost:$HTTP_PORT/api/v1/actors/$APP_ID/$actor/ask?timeout=$timeout" \
        -H "Content-Type: application/json" \
        ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
        -d "$payload" 2>/dev/null || echo '{"error":"timeout"}'
}

# Verify actors are alive
echo "Step 3: Verify actors"
echo "----------------------------------------------------------------"

# Check parameter server
PS_RESP=$(send_op "parameter-server" '{"op":"get_weights"}')
if echo "$PS_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}Parameter server OK${NC}"
else
    echo -e "  ${RED}Parameter server failed: $PS_RESP${NC}"
    exit 1
fi

# Check a worker
W0_RESP=$(send_op "data-worker-0" '{"op":"stats"}')
if echo "$W0_RESP" | grep -q '"status":"ok"'; then
    echo -e "  ${GREEN}Data workers OK${NC}"
else
    echo -e "  ${YELLOW}Data worker-0: $W0_RESP${NC}"
fi
echo ""

# Run training
echo "Step 4: Synchronous Parameter Server Training ($TRAIN_ITERATIONS iterations)"
echo "----------------------------------------------------------------"
echo ""

TRAIN_START=$(date +%s%N)

TRAIN_RESP=$(send_op "parameter-server" "{\"op\":\"train\",\"iterations\":$TRAIN_ITERATIONS}" 120)

TRAIN_END=$(date +%s%N)
WALL_MS=$(( (TRAIN_END - TRAIN_START) / 1000000 ))

if echo "$TRAIN_RESP" | grep -q '"status":"ok"'; then
    ITERS=$(echo "$TRAIN_RESP" | python3 -c "import sys,json; d=json.load(sys.stdin); p=d.get('payload',d); print(p.get('iterations_completed',0))" 2>/dev/null || echo "?")
    echo -e "  ${GREEN}Training complete: $ITERS iterations${NC}"

    # Print per-iteration results
    echo ""
    echo "$TRAIN_RESP" | python3 -c "
import sys, json
d = json.load(sys.stdin)
p = d.get('payload', d)  # unwrap HTTP envelope
results = p.get('results', [])
for r in results:
    it = r.get('iteration', '?')
    loss = r.get('loss', 0)
    coord = r.get('coord_ms', 0)
    comp = r.get('compute_ms', 0)
    w = r.get('workers', 0)
    if isinstance(loss, str): loss = float(loss)
    if isinstance(coord, str): coord = float(coord)
    if isinstance(comp, str): comp = float(comp)
    total = coord + comp
    pct = (comp / total * 100) if total > 0 else 0
    print(f'  Iter {it:>3}: loss={loss:.6f}  coord={coord:>6.0f}ms  compute={comp:>6.0f}ms  efficiency={pct:.0f}%  workers={w}')
" 2>/dev/null || echo "  (could not parse iteration details)"
else
    echo -e "  ${RED}Training failed: $TRAIN_RESP${NC}"
fi
echo ""

# Get final stats with benchmarks
echo "Step 5: Final Statistics & Benchmarks"
echo "----------------------------------------------------------------"

STATS_RESP=$(send_op "parameter-server" '{"op":"stats"}')

if echo "$STATS_RESP" | grep -q '"status":"ok"'; then
    echo "$STATS_RESP" | python3 -c "
import sys, json

raw = json.load(sys.stdin)
d = raw.get('payload', raw)  # unwrap HTTP envelope
model = d.get('model', {})
training = d.get('training', {})
bench = d.get('benchmarks', {})

def f(v):
    if isinstance(v, str):
        try: return float(v)
        except: return 0.0
    return float(v) if v else 0.0

print()
print('  Model')
print('  -----')
print(f'  Architecture:     {model.get(\"arch\", \"?\")}')
print(f'  Total parameters: {model.get(\"params\", 0):,}')
print(f'  W1 mean:          {f(model.get(\"w1_mean\", 0)):.6f}')
print(f'  W2 mean:          {f(model.get(\"w2_mean\", 0)):.6f}')
print()
print('  Training')
print('  --------')
print(f'  Iterations:       {training.get(\"iterations\", 0)}')
print(f'  Learning rate:    {f(training.get(\"lr\", 0))}')
print(f'  Workers:          {training.get(\"workers\", 0)}')
print()

total_ms = f(bench.get('total_ms', 0))
coord_ms = f(bench.get('coord_ms', 0))
compute_ms = f(bench.get('compute_ms', 0))
coord_pct = f(bench.get('coord_pct', 0))
compute_pct = f(bench.get('compute_pct', 0))
granularity = f(bench.get('granularity', 0))
params_sec = f(bench.get('params_per_sec', 0))
gflops = f(bench.get('gflops', 0))

print('  ================================================================')
print('  Coordination vs Computation Metrics')
print('  ================================================================')
print(f'  Total time:       {total_ms:>10,.0f} ms')
print(f'  Coordination:     {coord_ms:>10,.0f} ms  ({coord_pct:.1f}%)')
print(f'  Computation:      {compute_ms:>10,.0f} ms  ({compute_pct:.1f}%)')
print(f'  Granularity:      {granularity:>10.2f}x   (compute/coordinate)')
print()
print('  Benchmark Metrics')
print('  -----------------')
print(f'  Throughput:       {params_sec:>10,.0f} params/sec')
print(f'  GFLOPS:           {gflops:>10.4f}')
print()
" 2>/dev/null || echo "  (could not parse stats)"
else
    echo -e "  ${YELLOW}Stats: $STATS_RESP${NC}"
fi

echo "================================================================"
echo ""
echo "  Wall clock time: ${WALL_MS}ms"
echo ""
echo "  This example demonstrated:"
echo "    - Multi-actor ApplicationSpec (ParameterServer + DataWorker)"
echo "    - Inter-actor messaging via host.ask() (request-reply)"
echo "    - Distributed gradient aggregation (synchronous SGD)"
echo "    - Coordination vs computation benchmarking"
echo ""
echo "  Ray equivalent: @ray.remote class + ray.get(futures)"
echo "  PlexSpaces:     @actor class + host.ask(worker, op, payload)"
echo ""
echo "================================================================"
