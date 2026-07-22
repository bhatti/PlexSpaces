#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Integration test for the mersenne_prime distributed computation example.
#
# Two Node.js WsThinClient workers drive CoordinatorActor through the first
# CANDIDATE_COUNT exponents (default 8).  The test verifies that all known
# Mersenne prime exponents in that range are found.
#
# Also prints a scaling benchmark: wall-clock time with 1 and 2 workers,
# showing parallelism speedup.
#
# Usage:
#   bash test.sh [port]
#
# Environment overrides:
#   WS_PORT, WS_URL, HTTP_URL, LEADER_NODE_ID, TIMEOUT (seconds),
#   CANDIDATE_COUNT (default 8 — all known primes in first 8 candidates)
#   KEEP_DEPLOYED=0   undeploy at the end (default is to keep it deployed)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
SDK_DIR="$REPO_ROOT/sdks/typescript/dist"
TARGET_DIR="$REPO_ROOT/target/examples/typescript/mersenne_prime"
OUTPUT_WASM="$TARGET_DIR/mersenne_actor.wasm"

WS_PORT="${1:-${WS_PORT:-8091}}"
WS_URL="${WS_URL:-ws://localhost:${WS_PORT}/ws}"
HTTP_URL="${HTTP_URL:-http://localhost:${WS_PORT}}"
APP_ID="ts-mersenne-prime"
LEADER_NODE_ID="${LEADER_NODE_ID:-test-node-${WS_PORT}}"
TIMEOUT="${TIMEOUT:-60}"
CANDIDATE_COUNT="${CANDIDATE_COUNT:-8}"  # [2,3,5,7,13,17,19,31] — all known primes

log() { echo "[test.sh] $*" >&2; }
fail() { echo "[FAIL] $*" >&2; exit 1; }

# ─── 0. Build ─────────────────────────────────────────────────────────────────
log "Building mersenne_prime…"
cd "$SCRIPT_DIR"
bash build.sh
[ -f "$OUTPUT_WASM" ]             || fail "WASM not built: $OUTPUT_WASM"
[ -f "$SDK_DIR/ws_thin_client.js" ] || fail "TypeScript SDK not built. Run: cd $REPO_ROOT/sdks/typescript && npm run build"

# ─── 1. JWT auth (auto-generate if server has auth enabled) ───────────────────
export AUTH_HEADER="${AUTH_HEADER:-}"
if [ -z "${PLEXSPACES_TEST_TOKEN:-}" ] && [ -f "$REPO_ROOT/scripts/gen-test-jwt.sh" ]; then
  source ~/venv/bin/activate 2>/dev/null || true
  JWT_OUTPUT="$(PLEXSPACES_JWT_PRIVATE_KEY_FILE="$REPO_ROOT/certs/jwt-es256.pem" "$REPO_ROOT/scripts/gen-test-jwt.sh" 2>/dev/null)" || true
  eval "$JWT_OUTPUT" 2>/dev/null || true
fi
if [ -n "${PLEXSPACES_TEST_TOKEN:-}" ]; then
  AUTH_HEADER="Authorization: Bearer $PLEXSPACES_TEST_TOKEN"
fi
WS_TOKEN="${PLEXSPACES_TEST_TOKEN:-}"

# ─── 2. Check node ────────────────────────────────────────────────────────────
log "Checking node at ${HTTP_URL}…"
trap 'rm -f "${APP_ZIP:-}"' EXIT
APP_ZIP="$(mktemp /tmp/app_XXXXXX.zip)"
rm -f "$APP_ZIP"
zip -j "$APP_ZIP" "${OUTPUT_WASM}" "${SCRIPT_DIR}/app-config.toml" >/dev/null
(cd "${SCRIPT_DIR}" && zip "$APP_ZIP" static/ static/* 2>/dev/null || true)
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "${HTTP_URL}/health" 2>/dev/null || echo "000")
[ "$HTTP_CODE" = "200" ] || fail "Node not reachable (HTTP $HTTP_CODE). Start with: ./scripts/server.sh"

# ─── 3. Undeploy any previous run, then deploy ────────────────────────────────
bash "$SCRIPT_DIR/undeploy.sh" "$WS_PORT" 2>/dev/null || true
sleep 1

log "Deploying ${APP_ID}…"
_deployed=0
for _attempt in 1 2 3; do
  DEPLOY_OUT=$(curl -s -w "\n%{http_code}" -X POST "${HTTP_URL}/api/v1/applications/deploy" \
    ${AUTH_HEADER:+-H "$AUTH_HEADER"} \
    -F "application_id=${APP_ID}" \
    -F "name=${APP_ID}" \
    -F "version=1.0.0" \
    -F "app_file=@$APP_ZIP" 2>&1) || true
  HTTP_CODE=$(echo "$DEPLOY_OUT" | tail -n1)
  RESPONSE=$(echo "$DEPLOY_OUT" | sed '$d')
  if [ "$HTTP_CODE" = "200" ] && echo "$RESPONSE" | grep -qE '"success"[[:space:]]*:[[:space:]]*true'; then
    _deployed=1
    break
  fi
  log "Deploy attempt $_attempt failed (HTTP $HTTP_CODE), retrying…"
  sleep 3
done
[ "$_deployed" -eq 1 ] || fail "Deploy failed after 3 attempts: $RESPONSE"
log "Deployed OK"

# ─── 4. Run benchmark: 1 worker, then 2 workers ───────────────────────────────
log "Running scaling benchmark (1 worker, then 2 workers)…"

run_workers() {
  local NUM_WORKERS="$1"
  WS_URL="$WS_URL" LEADER_NODE_ID="$LEADER_NODE_ID" \
  TIMEOUT="$TIMEOUT" SDK_DIR="$SDK_DIR" WS_TOKEN="$WS_TOKEN" \
  CANDIDATE_COUNT="$CANDIDATE_COUNT" NUM_WORKERS="$NUM_WORKERS" \
  node --input-type=module <<'NODEOF'
import { pathToFileURL } from "node:url";

// Import only the thin-client modules — avoids plexspaces: WIT host imports
// that exist in actor.js/host.js and cannot be resolved outside WASM.
const SDK_DIR = process.env.SDK_DIR;
const { WsThinClient } = await import(pathToFileURL(SDK_DIR + "/ws_thin_client.js").href);
const { ActorID }      = await import(pathToFileURL(SDK_DIR + "/actor_id.js").href);

const WS_URL         = process.env.WS_URL          || "ws://localhost:8091/ws";
const WS_TOKEN       = process.env.WS_TOKEN        || "";
const LEADER_NODE_ID = process.env.LEADER_NODE_ID  || "test-node-8091";
const APP_NS         = "ts-mersenne-prime";
const TIMEOUT_MS     = parseInt(process.env.TIMEOUT || "120") * 1000;
const COUNT          = parseInt(process.env.CANDIDATE_COUNT || "8");
const NUM_WORKERS    = parseInt(process.env.NUM_WORKERS || "1");

const EXPECTED_PRIMES = [2, 3, 5, 7, 13, 17, 19, 31].slice(0, COUNT);

// Lucas-Lehmer with BigInt (no Web Worker in Node.js, runs inline)
function lucasLehmer(p) {
  const bp = BigInt(p);
  if (bp === 2n) return true;
  const mp = (1n << bp) - 1n;
  let s = 4n;
  for (let i = 0n; i < bp - 2n; i++) s = (s * s - 2n) % mp;
  return s === 0n;
}

function mkClient() {
  const opts = { wsUrl: WS_URL, nodeId: WsThinClient.newUlid(), namespace: APP_NS };
  if (WS_TOKEN) opts.jwtToken = WS_TOKEN;
  return new WsThinClient(opts);
}

const coordinatorId = new ActorID("mersenne", "CoordinatorActor", APP_NS, LEADER_NODE_ID).toString();

async function runWorker(workerId) {
  const c = mkClient();
  await c.connect();
  const myActorId = c.localActorId(`worker${workerId}`, "WorkerNode", APP_NS);
  let completed = 0;

  const done = new Promise(resolve => {
    c.onMessage((_from, msgType, payload) => {
      if (msgType === "assign_work") {
        const { p, done: isDone } = payload;
        if (isDone || p == null) { resolve(completed); return; }
        const t0w = Date.now();
        const is_prime = lucasLehmer(p);
        const duration_ms = Date.now() - t0w;
        completed++;
        c.tell(coordinatorId, "result", { p, is_prime, duration_ms, actor_id: myActorId }).catch(() => {});
      }
    });
  });

  // join first, then start dispatches work to all registered workers
  await c.ask(coordinatorId, "join", { actor_id: myActorId, cpu_cores: 2 }, 10_000);
  const n = await Promise.race([done, new Promise((_, rej) => setTimeout(() => rej(new Error("worker timeout")), TIMEOUT_MS))]);
  await c.disconnect();
  return n;
}

// Register all workers, then start the run (they receive work immediately on start)
const workers = Array.from({ length: NUM_WORKERS }, (_, i) => mkClient());
await Promise.all(workers.map(c => c.connect()));
const workerActorIds = workers.map((c, i) => c.localActorId(`worker${i+1}`, "WorkerNode", APP_NS));

// Set up message handlers before start
const allDone = Promise.all(workers.map((c, i) => {
  const myActorId = workerActorIds[i];
  let completed = 0;
  return new Promise(resolve => {
    c.onMessage((_from, msgType, payload) => {
      if (msgType === "assign_work") {
        const { p, done: isDone } = payload;
        if (isDone || p == null) { resolve(completed); return; }
        const t0w = Date.now();
        const is_prime = lucasLehmer(p);
        const duration_ms = Date.now() - t0w;
        completed++;
        c.tell(coordinatorId, "result", { p, is_prime, duration_ms, actor_id: myActorId }).catch(() => {});
      }
    });
  });
}));

// Join all workers first
await Promise.all(workers.map((c, i) =>
  c.ask(coordinatorId, "join", { actor_id: workerActorIds[i], cpu_cores: 2 }, 10_000)
));

// Start the run — coordinator dispatches immediately to all joined workers
const starter = mkClient();
await starter.connect();
await starter.ask(coordinatorId, "start", { count: COUNT }, 10_000);

const t0 = Date.now();
await Promise.race([
  allDone,
  new Promise((_, rej) => setTimeout(() => rej(new Error("overall timeout")), TIMEOUT_MS)),
]);
const elapsed = Date.now() - t0;

const status = await starter.ask(coordinatorId, "status", {}, 10_000);
await starter.disconnect();
await Promise.all(workers.map(c => c.disconnect()));

const foundPrimes = (status.found_primes ?? []).sort((a, b) => a - b);
for (const p of EXPECTED_PRIMES) {
  if (!foundPrimes.includes(p)) {
    console.error(`[FAIL] Expected 2^${p}-1 to be identified as prime but not found`);
    process.exit(1);
  }
}

console.log("BENCH=" + JSON.stringify({
  workers: NUM_WORKERS,
  candidates: COUNT,
  found_primes: foundPrimes.length,
  elapsed_ms: elapsed,
}));
NODEOF
}

BENCH1=$(run_workers 1 | grep '^BENCH=' | sed 's/BENCH=//')
log "1-worker run complete"

BENCH2=$(run_workers 2 | grep '^BENCH=' | sed 's/BENCH=//')
log "2-worker run complete"

# ─── 5. Print benchmark table ─────────────────────────────────────────────────
python3 - "$BENCH1" "$BENCH2" <<'PYEOF'
import sys, json

runs = [json.loads(a) for a in sys.argv[1:] if a]
if not runs:
    sys.exit(0)

print()
print("┌─────────────────────────────────────────────────────────────────────────────┐")
print("│              mersenne_prime  Distributed Scaling Benchmark                  │")
print("│  (Lucas-Lehmer BigInt, candidates 2–31, browser workers via WsThinClient)   │")
print("├─────────┬────────────┬──────────────┬───────────┬──────────────┬────────────┤")
print("│ Workers │ Candidates │ Found Primes │ Elapsed s │  Throughput  │  Speedup   │")
print("├─────────┼────────────┼──────────────┼───────────┼──────────────┼────────────┤")
base_elapsed = runs[0]["elapsed_ms"] if runs else 1
for r in runs:
    elapsed_s = r["elapsed_ms"] / 1000
    throughput = r["candidates"] / elapsed_s if elapsed_s > 0 else 0
    speedup = base_elapsed / r["elapsed_ms"] if r["elapsed_ms"] > 0 else 1.0
    efficiency = speedup / r["workers"] * 100
    print(f"│ {r['workers']:>7} │ {r['candidates']:>10} │ {r['found_primes']:>12} │ {elapsed_s:>9.2f} │ {throughput:>10.2f}/s │ {speedup:>5.2f}x ({efficiency:.0f}%) │")
print("└─────────┴────────────┴──────────────┴───────────┴──────────────┴────────────┘")
print()
print("Routing path: WsThinClient → WS endpoint → CoordinatorActor.onReady()")
print("              → host.send(worker_actor_id, 'assign_work') → thin-node session")
print("              → lucasLehmer(BigInt) → host.send(coord, 'result')")
print("              → host.ts.write(['result', ...]) + host.incrCounter()")
PYEOF

# ─── 6. Undeploy (skipped if KEEP_DEPLOYED=1) ─────────────────────────────────
if [ "${KEEP_DEPLOYED:-1}" = "1" ]; then
  log "KEEP_DEPLOYED=1 — skipping undeploy"
  log "Browser UI available at: ${HTTP_URL}/apps/${APP_ID}/"
else
  log "Undeploying ${APP_ID}…"
  bash "$SCRIPT_DIR/undeploy.sh" "$WS_PORT"
fi

log "ALL TESTS PASSED"
