# PlexSpaces Load Tests

Purpose-built actors in all five supported languages, k6 test scripts covering HTTP, per-language server isolation, CPU flamegraph capture, a memory watchdog, and report scripts for tracking performance over time.

The goal is not just "does it pass?" — it is to find where the system slows down, capture a flamegraph showing why, and track improvement over time.

---

## Directory Layout

```
load-test/
├── build.sh                      Build all artifacts (release); skips up-to-date binaries
├── docs/
│   ├── quickstart.md             ← Start here: run → analyze → compare → fix cycle
│   └── test-plan.md              Full test plan: goals, test cases, acceptance criteria
├── apps/
│   ├── python/apps/perf_actor/   Python WASM actor
│   ├── go/apps/perf_actor/       Go (TinyGo) WASM actor
│   ├── typescript/apps/perf_actor/  TypeScript WASM actor
│   ├── rust/apps/perf_actor/     Rust WASM actor
│   └── rust/embedded/perf_actor/ Rust embedded actor (native Tokio — the performance baseline)
├── k6/
│   ├── common.js                 Shared helpers (auth, URLs, custom metrics)
│   ├── run.sh                    Main runner — builds, deploys, runs, reports
│   └── scenarios/
│       ├── echo_http.js          HTTP echo round-trip per language
│       ├── kv_http.js            KV put+get via HTTP
│       ├── spawn_lifecycle.js    Spawn → call → stop lifecycle
│       ├── ws_connections.js     WS capacity ramp via k6 (staged 50→2000)
│       ├── process_group.js      ProcessGroup create/join/broadcast/delete
│       ├── shard_group.js        ShardGroup HPC scatter-gather/all-reduce
│       ├── elastic_pool.js       ElasticPool checkout/compute/checkin
│       └── actor_capacity.js     Max actors capacity via k6
├── scripts/                      Custom Node.js capacity scripts (no k6 dependency)
│   ├── package.json              npm manifest (ws dependency)
│   ├── actor_capacity.js         Spawn actors until RSS limit — N concurrent workers
│   └── ws_capacity.js            Open WS connections until RSS limit — batched ramp
├── profiling/
│   └── capture.sh                CPU flamegraph capture (samply/perf/macOS sample)
├── monitoring/
│   ├── collect.sh                Writes RSS/CPU/actor metrics to CSV every 5s (used by run.sh)
│   └── monitor.sh                Live terminal dashboard — refreshes every N seconds
└── reports/
    ├── analyze.sh                Analyze one run: per-language deep-dive, cross-language comparison, per-scenario timestamps
    ├── compare.sh                Diff two runs and flag regressions
    └── template.md               Report template (one per run)
```

Reports land in **`load-test-data/<run-id>/lang-<name>/`** (gitignored) — one subdirectory per language, each with its own server log, metrics CSV, and k6 output files.

---

## Prerequisites

### Required

| Tool | Install |
|------|---------|
| k6 v2.1+ | macOS: `brew install k6` · Linux: see [k6 install docs](https://k6.io/docs/get-started/installation/) |

Build all artifacts (only rebuilds what's stale):
```bash
bash load-test/build.sh          # everything
bash load-test/build.sh server embedded   # native binaries only
bash load-test/build.sh python go         # WASM actors only
```

Running `build.sh` again is safe — it skips anything already up to date. To force a specific rebuild: `rm target/release/perf_actor` then re-run `build.sh embedded`.

### Per-language (WASM actors only)

| Language | Tools needed |
|----------|-------------|
| Python WASM | Python venv with plexspaces SDK: `pip install -e sdks/python` |
| Go WASM | TinyGo (`brew install tinygo`), wasm-tools, wasm-opt |
| TypeScript WASM | Node.js 20+, jco (`npm install -g @bytecodealliance/jco`) |
| Rust WASM | `rustup target add wasm32-wasip1`, wasm-tools |

### Node.js scripts (`scripts/`)

The `ws-capacity` and `actor-capacity` modes use plain Node.js scripts instead of k6. Install once before running them:

```bash
npm --prefix load-test/scripts install
```

No additional install is needed when using `run.sh` — it auto-installs on first run.

### Optional (flamegraphs)

| Tool | Platform | Purpose |
|------|----------|---------|
| `cargo install samply` | macOS | Best option — Firefox Profiler JSON |
| `perf` + `cargo install inferno` | Linux | SVG flamegraph from perf data |
| `sample` | macOS (built-in) | Fallback — no install needed |

---

## Quick Start

### Fastest path — `--start-server` (build + start + test + stop in one command)

`--start-server` handles the full lifecycle per language: wipe `~/plexspaces` state, start a fresh server, deploy the WASM app, warm up, run k6, then stop the server. Each language runs in complete isolation — no shared state.

```bash
# Smoke test — all 5 languages, 30s, 10 VUs (verify nothing is broken)
bash load-test/k6/run.sh --start-server --no-auth --mode all --lang all --duration 30s --vus 10

# Full baseline — all 5 languages, 2 min, 100 VUs
bash load-test/k6/run.sh --start-server --no-auth --mode all --lang all --duration 2m --vus 100

# Single language — Rust WASM only
bash load-test/k6/run.sh --start-server --no-auth --mode all --lang rust-wasm --duration 2m --vus 100

# Reports land in load-test-data/<run-id>/lang-<name>/
ls load-test-data/
```

### Per-language isolation

Each language test follows this sequence automatically (when `--start-server` is set):
1. Stop any running server
2. Wipe `~/plexspaces/apps` and `~/plexspaces/db/` (fresh DB, no stale registrations)
3. Start a clean server process
4. Deploy and activate the WASM app (or start the embedded actor binary)
5. Warm up actor instances
6. Run k6 and collect metrics into `lang-<name>/`
7. Stop the server

This ensures Go, TypeScript, and Python tests never contaminate each other's memory or DB state.

### Manual start (pre-built binaries)

```bash
# 1. Build everything that's stale (safe to re-run; skips up-to-date artifacts)
bash load-test/build.sh

# 2. Start embedded actor (no auth)
PLEXSPACES_DISABLE_AUTH=1 PERF_WARM_COUNT=10 RUST_LOG=info target/release/perf_actor 8092 &

# 3. Start main WASM server (no auth)
PLEXSPACES_DISABLE_AUTH=1 RUST_LOG=info target/release/plexspaces start \
  --node-id load-test-node-8091 --listen-addr 0.0.0.0:8091 &

# 4. Run tests (no per-lang isolation — you manage server lifecycle)
bash load-test/k6/run.sh --mode kv --lang rust-embedded --no-auth --duration 2m

# 5. Analyze results
bash load-test/reports/analyze.sh
```

### With auth (production-like)

```bash
PLEXSPACES_JWT_PUBLIC_KEY_FILE=certs/jwt-es256-pub.pem \
  PERF_WARM_COUNT=10 RUST_LOG=info target/release/perf_actor 8092 &
PLEXSPACES_JWT_PUBLIC_KEY_FILE=certs/jwt-es256-pub.pem \
  RUST_LOG=info target/release/plexspaces start \
  --node-id load-test-node-8091 --listen-addr 0.0.0.0:8091 &

bash load-test/k6/run.sh --mode echo --lang rust-embedded --duration 30s --vus 10
```

---

## Scalability Tests

> **Capacity tests are NOT included in `--mode all`.** They are destructive (OOM the server intentionally), long-running (up to 60 min), and must be run explicitly. Use `--mode max-actors`, `--mode ws-connections`, `--mode ws-capacity`, or `--mode actor-capacity`.

Four tests exist specifically to find the maximum capacity of the system — not throughput. Two use k6 (staged ramps, report thresholds); two use plain Node.js scripts (continuous loop until RSS limit, no k6 required). No artificial inter-scenario cooldown delays are inserted for these tests.

| Mode | Engine | Strategy |
|------|--------|----------|
| `ws-connections` | k6 | Staged ramp 50→2000 VUs, each level held for `STAGE_DURATION` |
| `max-actors` | k6 | rust-embedded: OOM ramp (60 min); WASM langs: 3-actor sanity check only |
| `ws-capacity` | Node.js | Batch-open connections every `RAMP_INTERVAL_MS` until RSS limit |
| `actor-capacity` | Node.js | N concurrent workers spawning actors until RSS limit (rust-embedded only) |

### Max WebSocket Connections — k6 (`--mode ws-connections`, rust-embedded only)

Finds how many simultaneous thin-node WebSocket connections the embedded Rust actor can hold.

Each k6 VU:
1. Upgrades to WebSocket (`/ws`)
2. Sends `WsFrame{node_register: NodeRegistration{node_role: THIN}}` (binary protobuf)
3. Waits for `WsFrame{node_register_ack}` from the server
4. Holds the connection open, sending heartbeats every `HB_INTERVAL_MS`
5. Closes cleanly after `STAGE_DURATION`

Scale ramp: **50 → 100 → 250 → 500 → 1000 → 2000** connections, each level held for `STAGE_DURATION` (default 30s).

```bash
# Default: ramp to 2000 thin-node connections, 30s per level
bash load-test/k6/run.sh --mode ws-connections --no-auth

# Shorter stages for quick testing
bash load-test/k6/run.sh --mode ws-connections --no-auth --duration 10s

# Cap the ramp at 500 connections
k6 run -e MAX_LEVEL=500 load-test/k6/scenarios/ws_connections.js
```

**Key env vars for ws_connections.js**:

| Var | Default | Purpose |
|-----|---------|---------|
| `STAGE_DURATION` | `30s` | Hold each level for this long before stepping up |
| `HB_INTERVAL_MS` | `5000` | Milliseconds between heartbeats per connection |
| `CONN_TIMEOUT_S` | `10` | WebSocket upgrade timeout |
| `MAX_LEVEL` | `2000` | Highest VU count to ramp to |

**Prometheus metrics tracked**:
- `plexspaces_ws_thin_nodes_active` — current active thin-node WS connections (gauge, emitted on connect/disconnect)
- `plexspaces_ws_thin_node_registered_total` — lifetime registered count
- `plexspaces_ws_thin_node_unregistered_total` — lifetime unregistered count

All are available at `/api/v1/dashboard/metrics-table` (JSON). The `handleSummary` function reads `plexspaces_ws_thin_nodes_active` after the test to show the server's final count.

**Report columns explained**:
- `max_concurrent_thin_nodes` — k6-tracked peak (VU-side running counter; `values.max` = peak seen)
- `server_active_post_test` — server-side `plexspaces_ws_thin_nodes_active` gauge, read post-test (should be 0 after teardown)
- `upgrade_error_rate` — >10% = server rejecting new WS connections (capacity ceiling reached)
- `unexpected_drop_rate` — server closing connections that the test did not close

---

### Max Actors — k6 (`--mode max-actors`, eager virtual actors)

Finds the maximum number of actors the node can hold simultaneously before hitting the memory limit.

**Actor modes**:
- **eager** (default for `--actor-type both`): each actor is a distinct named instance, activated on first request, never evicted. Every spawned actor stays alive. This is the true capacity test — counts ALL live actors.
- **virtual**: actors are managed by the VirtualActorManager with LRU eviction. Controlled by `PERF_MAX_VIRTUAL_POOL`. Set to 0 for unlimited (max-actors mode default).

```bash
# Default: eager + virtual, unlimited pool, rust-embedded
bash load-test/k6/run.sh --mode max-actors --no-auth

# Eager only (no eviction, clearest capacity measurement)
bash load-test/k6/run.sh --mode max-actors --no-auth --actor-type eager

# Higher memory limit (default is 2GB; 4GB for big machines)
k6 run -e MEMORY_LIMIT_MB=4096 load-test/k6/scenarios/actor_capacity.js

# Virtual actors with explicit pool cap (e.g. cap at 10000)
k6 run -e ACTOR_MODE=virtual -e PERF_MAX_VIRTUAL_POOL=10000 load-test/k6/scenarios/actor_capacity.js
```

**Key env vars for actor_capacity.js**:

| Var | Default | Purpose |
|-----|---------|---------|
| `ACTOR_MODE` | `eager` | `eager` = no eviction; `virtual` = LRU-evicted virtual actors |
| `BATCH` | `100` | Actors spawned per iteration |
| `MEMORY_LIMIT_MB` | `2048` | Stop when server RSS exceeds this (in MB) |
| `MAX_BATCHES` | `200` | Safety ceiling: stop after this many batches regardless |
| `SLEEP_BETWEEN_BATCHES_S` | `0.5` | Pause between batches to let RSS settle |
| `LANG` | `rust-embedded` | Target language (`rust-embedded`, `go`, `typescript`, `python`, `rust-wasm`) |

**Key env vars for the embedded actor binary (`perf_actor`)**:

| Var | Default | Purpose |
|-----|---------|---------|
| `PERF_MAX_VIRTUAL_POOL` | `100` | Max live virtual actor instances per type (LRU cap). Set `0` for unlimited. |
| `PERF_WARM_COUNT` | `10` | Number of actors pre-spawned at startup |

> **`max-actors` mode automatically sets `PERF_MAX_VIRTUAL_POOL=0`** (unlimited) so virtual actors accumulate without eviction. This is how 2000+ actors are observed. Regular `echo`/`kv` tests leave it at the default 100 to avoid runaway memory growth.

**Prometheus metrics tracked**:
- `plexspaces_node_active_actors` — total live actors on the node (gauge, updated on spawn/stop)
- `plexspaces_actor_active` — per-namespace active count
- `plexspaces_actor_active_total` — per-type, per-namespace breakdown
- `plexspaces_actor_spawn_total` — lifetime spawn count
- `plexspaces_node_actors_spawned_total` — per-node spawn counter

All readable via `/api/v1/dashboard/metrics-table` or `/api/v1/dashboard/nodes` (includes `active_actors` per node in heartbeat data).

**Report columns explained** (eager mode):
- `max_actors_spawned` — k6-tracked peak (actors confirmed spawned via 200 responses)
- `peak_rss` — server RSS at peak actor count
- `overhead_per_actor` — `(peak_rss - baseline_rss) / actors` in KB
- `vs_erlang_process` — overhead ratio vs Erlang's ~0.3KB per process

**Report columns explained** (virtual mode):
- `requested_total` — every POST attempt (even if evicted and re-activated)
- `live_at_peak` — maximum `plexspaces_node_active_actors` seen from the server
- `live_at_end` — final server-reported active count
- `evictions_detected` — `requested_total - live_at_end` (clamped to 0 if live ≥ requested)

---

### Max WebSocket Connections — Node.js (`--mode ws-capacity`, rust-embedded only)

Opens WebSocket connections in batches, ramping up until the server RSS exceeds the memory limit. Uses the same binary protobuf thin-node registration handshake as `ws-connections`. Each open connection sends periodic heartbeats.

```bash
# Default: batch 200, ramp every 300ms, stop at 90% of --mem-limit (default 2GB)
bash load-test/k6/run.sh --mode ws-capacity --no-auth --start-server

# Larger machine — 4GB limit, faster ramp
bash load-test/k6/run.sh --mode ws-capacity --no-auth --start-server --mem-limit 4096

# Run directly (server must already be running)
cd load-test/scripts
WS_URL=ws://localhost:8092/ws BATCH_SIZE=500 RAMP_INTERVAL_MS=200 node ws_capacity.js
```

**Key env vars for `ws_capacity.js`**:

| Var | Default | Purpose |
|-----|---------|---------|
| `WS_URL` | `ws://localhost:8092/ws` | WebSocket endpoint |
| `TARGET` | `50000` | Maximum connections to attempt |
| `BATCH_SIZE` | `100` | Connections opened per ramp step |
| `RAMP_INTERVAL_MS` | `500` | Delay between batches (ms) |
| `HB_INTERVAL_MS` | `10000` | Heartbeat period per connection (ms) |
| `HOLD_DURATION_S` | `10` | Hold at peak before closing (s) |
| `MEMORY_LIMIT_MB` | `2048` | Abort when server RSS exceeds this |
| `STOP_PERCENT` | `90` | Stop at this % of MEMORY_LIMIT |
| `POLL_INTERVAL_MS` | `3000` | Monitor poll interval (ms) |

**Report columns**:
- `max_concurrent_conns` — local count of simultaneously open connections
- `max_server_ws_active` — `plexspaces_ws_thin_nodes_active` Prometheus gauge peak
- `overhead_per_conn` — `(peak_rss - baseline_rss) / max_conns` in KB

---

### Max Actors — Node.js (`--mode actor-capacity`, all languages)

Spawns actors using N concurrent HTTP workers until the server RSS exceeds the memory limit. Actors are always **eager** (no LRU eviction) — every spawned actor stays alive. This gives the true capacity measurement.

```bash
# Default: 50 concurrent workers, 2GB limit, rust-embedded
bash load-test/k6/run.sh --mode actor-capacity --no-auth --start-server

# More aggressive — 100 workers, 4GB limit
bash load-test/k6/run.sh --mode actor-capacity --no-auth --start-server \
  --mem-limit 4096 --concurrency 100

# Run directly (server must already be running)
cd load-test/scripts
CONCURRENCY=100 MEMORY_LIMIT_MB=3000 node actor_capacity.js

# WASM language
cd load-test/scripts
LANG=go BASE_URL=http://localhost:8091 MEMORY_LIMIT_MB=512 node actor_capacity.js
```

**Key env vars for `actor_capacity.js`**:

| Var | Default | Purpose |
|-----|---------|---------|
| `LANG` | `rust-embedded` | Target language |
| `CONCURRENCY` | `50` | Parallel spawn workers |
| `MEMORY_LIMIT_MB` | `2048` | Abort when server RSS exceeds this |
| `STOP_PERCENT` | `90` | Stop at this % of MEMORY_LIMIT |
| `POLL_INTERVAL_MS` | `3000` | Monitor poll interval (ms) |
| `MAX_DURATION_S` | `1800` | Wall-clock timeout (30m) |
| `BASE_URL` | `http://localhost:8092` | Server URL |

**Report columns**:
- `max_live_actors` — server-reported peak active actor count
- `spawns_confirmed` — total HTTP 200 responses (actor activations)
- `overhead_per_actor` — `(peak_rss - baseline_rss) / live_actors` in KB
- `spawn_rps` — actor activations per second (at N concurrent workers)

> **Spawn throughput**: with `CONCURRENCY=50` and ~10ms server-side activation, expect ~2000–5000 actors/sec — far more than the old sequential 1-VU k6 design (~33/sec). Increase `CONCURRENCY` if the server can handle more parallelism.

---

## Perf Iteration Cycle

```bash
# Step 1: Establish a baseline
bash load-test/k6/run.sh \
  --start-server --no-auth \
  --mode all --lang all \
  --duration 2m --vus 100
# → Prints run-id at the end, e.g. 20260728-143000
BASELINE=load-test-data/20260728-143000

# Step 2: Analyze
bash load-test/reports/analyze.sh "$BASELINE"

# Step 3: Make your fix (edit Rust code, proto, etc.)

# Step 4: Run again with the same settings
bash load-test/k6/run.sh \
  --start-server --no-auth \
  --mode all --lang all \
  --duration 2m --vus 100
FIXED=load-test-data/20260728-160000

# Step 5: Compare — any metric that regressed >10% is flagged ⚠
bash load-test/reports/compare.sh "$BASELINE" "$FIXED"
```

---

## All Options

```
--mode   MODE        echo | kv | spawn | ws-connections | max-actors | all
                     ws-capacity     (Node.js) ramp WS connections to RSS limit
                     actor-capacity  (Node.js) spawn actors to RSS limit
--lang   LANG        rust-embedded | rust-wasm | go | typescript | python | all
                     "all" runs each language sequentially, fully isolated
--vus    N           Concurrent users for k6 scenarios (default: 100)
--concurrency N      Parallel workers for Node.js capacity scripts (default: 50)
--duration D         Duration per scenario: 30s, 2m, 5m, 30m (default: 2m)
--scale-levels       Ramp 100→500→1000→5000→10000 VUs × --duration each
--actor-type TYPE    regular | virtual | both (default: both)
--port   PORT        Main server port (default: 8091)
--embedded-port P    Embedded actor port (default: 8092)
--mem-limit MB       Memory limit in MB; passed to Node scripts as MEMORY_LIMIT_MB;
                     also kills k6 tests if server RSS >= this (default: 4096 = 4GB)
--start-server       Full per-language lifecycle: wipe state → start → deploy → test → stop
--warm-count N       Pre-warmed actors for embedded actor when --start-server (default: 10)
--profile            Capture CPU flamegraph every --profile-interval seconds (max 4 per run)
--profile-interval N Seconds between flamegraph captures (default: 30)
--no-deploy          Skip WASM actor deployment
--no-auth            Disable JWT auth for the embedded actor; WASM server always uses auth
--output FILE        Write JSON summary to FILE
```

### `--lang` values

| Value | What runs |
|-------|-----------|
| `rust-embedded` | Native Tokio actor — the performance baseline |
| `rust-wasm` | Rust compiled to WASM |
| `go` | TinyGo compiled to WASM |
| `typescript` | TypeScript compiled to WASM |
| `python` | Python via componentize-py |
| `all` | All five, one at a time, each fully isolated |

There is no `--lang rust` shorthand. Use `rust-embedded` or `rust-wasm` individually, or `all`.

---

## Output Directory Structure

Each run produces:

```
load-test-data/<run-id>/
├── lang-rust-embedded/
│   ├── server.log                Embedded actor stdout (RUST_LOG=info)
│   ├── metrics.csv               RSS/CPU/actor-count, one row per 5s
│   ├── snapshots/                Prometheus/metrics-table JSON per interval
│   │   └── snap_0001_....json
│   ├── kv_http_rust-embedded.log Full k6 output (raw k6 summary)
│   └── kv_http_rust-embedded.json  k6 JSON summary
├── lang-rust-wasm/
│   ├── server.log
│   ├── metrics.csv
│   ├── snapshots/
│   ├── kv_http_rust-wasm.log
│   └── kv_http_rust-wasm.json
├── lang-go/
│   └── ...
├── lang-typescript/
│   └── ...
└── lang-python/
    └── ...
```

---

## Analyzing a Run

```bash
# Analyze the latest run
bash load-test/reports/analyze.sh

# Analyze a specific run
bash load-test/reports/analyze.sh load-test-data/20260728-143000
```

Output:
- **Per-language section** — per-scenario start/end timestamps + elapsed duration, k6 p50/p95/p99/RPS/errors, RSS min/max/delta, CPU peak, server error count, WASM reinstantiation ratio
- **Cross-language comparison table** — every language vs the `rust-embedded` baseline, overhead shown as Nx slower

Each test scenario shows:
```
│  echo_http_rust-embedded        type=echo/regular     VUs=10    dur=30s
│    start=2026-01-01T00:00:00Z  end=2026-01-01T00:00:37Z  elapsed=37s
│    p50=1.2ms    p95=3.4ms    p99=–    RPS=450.0     Errors=0.00%
```

No Python, no jq. Requires only `awk`, `sed`, `python3` (for WASM snapshot parsing).

## Comparing Two Runs

```bash
bash load-test/reports/compare.sh \
  load-test-data/20260727-140000 \
  load-test-data/20260727-160000

# Stricter threshold:
bash load-test/reports/compare.sh ... --threshold 5
```

Compares p95 latency and RPS per scenario. Regression >10% (default) → `⚠ REGRESSION`. Improvements → `✓`. Exits with code 1 if any regressions (CI-friendly). Also prints RSS delta. No Python, no jq.

---

## Memory Watchdog

`monitoring/collect.sh` monitors server-side RSS (perf_actor + plexspaces) every 5 seconds. If it exceeds `--mem-limit` (default 4 GB), all k6 processes are killed and a sentinel file is written to the language's output directory. The next language test checks for it and aborts.

---

## Reading the Numbers

| Observation | Interpretation |
|-------------|---------------|
| rust-wasm 7–10× slower than rust-embedded | wasmtime#8943 Store-per-message cold start — every message recreates the WASM instance |
| go/typescript 10–25× slower | Same cold start + language interpreter overhead |
| python 40–50× slower | Componentize-py interpreter on top of WASM cold start |
| WASM reinstantiations = message count | 100% cold start — pre-warming helps warmup but wasmtime#8943 is structural |
| All WASM equally slow | WASM sandbox cost, not language |
| Memory flat under load | Healthy — no leak |
| Memory growing per request | Leak — check snapshots/ and flamegraph allocations |

---

## Viewing Flamegraphs

```bash
# macOS — open samply JSON at https://profiler.firefox.com (drag and drop)
ls load-test-data/*/flamegraph-*/cpu-profile-*.json

# Linux — open SVG in a browser
ls load-test-data/*/flamegraph-*/flamegraph-cpu-*.svg
```

Wide horizontal bars = hot code paths. Look for: unexpected allocators, lock contention, WASM interpreter, `prost` deserialization.

---

## Live Monitoring Dashboard

While a test is running, open a second terminal:

```bash
bash load-test/monitoring/monitor.sh --interval 5
```

Shows RSS/CPU for both servers, actor counts, all k6 processes, RSS trend, and watchdog status.

---

## Troubleshooting

**"JWT auth verification failed"**
```bash
# Disable auth entirely for local dev:
PLEXSPACES_DISABLE_AUTH=1 target/release/perf_actor 8092 &
bash load-test/k6/run.sh --no-auth ...
```

**"Stale actor registrations / 504 errors on WASM languages"**: `--start-server` wipes the DB automatically. If running manually, clean before restarting: `rm -f ~/plexspaces/db/plexspaces.db*`.

**"Memory watchdog fired immediately"**: Check for a stale server from a previous run: `pkill -f perf_actor && pkill -f "plexspaces start"`. Or raise the limit: `--mem-limit 6144`.

**"`LANG: en_US.UTF-8` in actor_capacity output"**: The Unix `LANG` locale env var shadows the k6 `-e LANG=rust-embedded` default when running k6 directly. Always pass `-e LANG=rust-embedded` explicitly when running actor_capacity.js or ws_connections.js by hand: `k6 run -e LANG=rust-embedded ...`. The `run.sh` script always passes `-e LANG=...` explicitly — this only affects direct k6 invocations.

**"k6 not found"**: `brew install k6` (macOS) or see [k6.io/docs/get-started/installation](https://k6.io/docs/get-started/installation/).

**"Flamegraph capture produces no output"**:
- macOS: `cargo install samply` (preferred) or use built-in `sample` (automatic fallback)
- Linux: `sudo apt install linux-tools-generic && cargo install inferno`
