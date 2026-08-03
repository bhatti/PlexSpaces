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
│       ├── ws_connections.js     Ramp WebSocket connections to find max capacity
│       ├── process_group.js      ProcessGroup create/join/broadcast/delete
│       ├── shard_group.js        ShardGroup HPC scatter-gather/all-reduce
│       ├── elastic_pool.js       ElasticPool checkout/compute/checkin
│       └── actor_capacity.js     Find max actors at memory limit
├── profiling/
│   └── capture.sh                CPU flamegraph capture (samply/perf/macOS sample)
├── monitoring/
│   ├── collect.sh                Writes RSS/CPU/actor metrics to CSV every 5s (used by run.sh)
│   └── monitor.sh                Live terminal dashboard — refreshes every N seconds
└── reports/
    ├── analyze.sh                Analyze one run: per-language deep-dive, cross-language comparison
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
--mode   MODE        echo | kv | spawn | ws-connections | all
--lang   LANG        rust-embedded | rust-wasm | go | typescript | python | all
                     "all" runs each language sequentially, fully isolated
--vus    N           Concurrent users (default: 100)
--duration D         Duration per scenario: 30s, 2m, 5m, 30m (default: 2m)
--scale-levels       Ramp 100→500→1000→5000→10000 VUs × --duration each
--actor-type TYPE    regular | virtual | both (default: both)
--port   PORT        Main server port (default: 8091)
--embedded-port P    Embedded actor port (default: 8092)
--mem-limit MB       Kill all tests if server RSS >= this (default: 4096 = 4GB)
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
- **Per-language section** — k6 p50/p95/p99/RPS/errors, RSS min/max/delta, CPU peak, server error count, WASM reinstantiation ratio
- **Cross-language comparison table** — every language vs the `rust-embedded` baseline, overhead shown as Nx slower

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

**"k6 not found"**: `brew install k6` (macOS) or see [k6.io/docs/get-started/installation](https://k6.io/docs/get-started/installation/).

**"Flamegraph capture produces no output"**:
- macOS: `cargo install samply` (preferred) or use built-in `sample` (automatic fallback)
- Linux: `sudo apt install linux-tools-generic && cargo install inferno`
