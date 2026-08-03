# Load Test Quick Start — Perf Iteration Guide

Goal: find bottlenecks, fix them, prove the fix. One command to run, one command to analyze, one command to compare.

---

## One-time setup

```bash
# Install k6 (macOS)
brew install k6

# Build everything (only rebuilds what's stale — safe to re-run)
bash load-test/build.sh

# Or build specific targets:
bash load-test/build.sh server embedded        # native binaries only (fastest)
bash load-test/build.sh python go typescript rust-wasm  # WASM actors
```

To force a rebuild of a specific artifact: `rm target/release/perf_actor` then `bash load-test/build.sh embedded`.

---

## Step 1 — Smoke test (30 seconds, verify nothing is broken)

```bash
bash load-test/k6/run.sh \
  --start-server --no-auth \
  --mode all --lang all \
  --duration 30s --vus 10
```

`--start-server` handles everything per language: wipes `~/plexspaces` state, starts a fresh server, warms up actors, runs k6, then stops. Each of the 5 languages runs sequentially in full isolation. At 30s / 10 VUs the whole smoke run completes in a few minutes.

**Pass criteria**: 0% errors across all 5 languages.

---

## Step 2 — Establish a baseline

Run each language sequentially, fully isolated (fresh server + clean DB per language):

```bash
bash load-test/k6/run.sh \
  --start-server --no-auth \
  --mode all --lang all \
  --duration 2m --vus 100
```

Output lands in `load-test-data/<run-id>/lang-<name>/` — one subdirectory per language with its own server log, metrics CSV, snapshots, and k6 output.

To run a single language:
```bash
bash load-test/k6/run.sh --start-server --no-auth --mode all --lang rust-embedded --duration 2m --vus 100
bash load-test/k6/run.sh --start-server --no-auth --mode all --lang go --duration 2m --vus 100
```

Valid `--lang` values: `rust-embedded`, `rust-wasm`, `go`, `typescript`, `python`, `all`.  
There is no `--lang rust` shorthand.

---

## Step 3 — Analyze the run

```bash
# Analyze the latest run
bash load-test/reports/analyze.sh

# Or specify a run
bash load-test/reports/analyze.sh load-test-data/20260728-143000
```

Prints a per-language section for each language that ran, then a cross-language comparison table:

```
┌─ rust-embedded ──────────────────────────────────────────
│  k6 kv_http_rust-embedded  p50=0ms  p95=1ms  RPS=1133892  Errors=0.00%
│  RSS(embedded): min=19MB max=20MB delta=1MB  CPU peak=31%
│  Server log: 0 errors  0 warnings
└──────────────────────────────────────────────────────────

╔═══════════════════════════ Language Comparison ══════════╗
║  rust-embedded  kv_http  p95=1ms   RPS=1133892  (baseline)    ║
║  rust-wasm      kv_http  p95=7ms   RPS=78363    7.0x slower   ║
║  go             kv_http  p95=11ms  RPS=47006    11.0x slower  ║
║  typescript     kv_http  p95=25ms  RPS=20846    25.0x slower  ║
║  python         kv_http  p95=45ms  RPS=11107    45.0x slower  ║
╚══════════════════════════════════════════════════════════╝
```

No Python, no jq required — only `awk`, `sed`, `python3`.

---

## Step 4 — Memory and WASM diagnostics

**RSS delta from metrics.csv** (delta should stay under 50 MB, not grow gigabytes):

```bash
awk -F',' 'NR>1 && $4!="" {
    if ($4+0 > max) max=$4+0
    if (min=="" || $4+0 < min) min=$4+0
  } END { printf "min=%.0fMB  max=%.0fMB  delta=%.0fMB\n", min, max, max-min }' \
  load-test-data/<run-id>/lang-<name>/metrics.csv
```

**Live dashboard (second terminal, while a test is running)**:

```bash
bash load-test/monitoring/monitor.sh --interval 5
```

---

## Step 5 — Find the bottleneck (scale ramp + flamegraph)

Start with rust-embedded — it isolates framework overhead from WASM cost:

```bash
bash load-test/k6/run.sh \
  --start-server --no-auth \
  --mode echo --lang rust-embedded \
  --scale-levels --duration 5m \
  --profile --profile-interval 60
```

Runs at 100 → 500 → 1000 → 5000 → 10000 VUs, 5 min each. Stops if the memory watchdog fires (default 4 GB). Flamegraphs captured every 60s.

**Open the flamegraph**:
```bash
# macOS — drag and drop to https://profiler.firefox.com
ls load-test-data/*/flamegraph-*/cpu-profile-*.json

# Linux — open SVG in browser
ls load-test-data/*/flamegraph-*/flamegraph-cpu-*.svg
```

Wide horizontal bars = hot code paths. Look for: unexpected allocators, lock contention, WASM interpreter, `prost` deserialization.

---

## Step 6 — Fix, re-run, compare

```bash
# After your fix:
bash load-test/k6/run.sh \
  --start-server --no-auth \
  --mode all --lang all \
  --duration 2m --vus 100
# → new run-id e.g. 20260728-160000

# Compare — regressions >10% flagged ⚠, improvements marked ✓
bash load-test/reports/compare.sh \
  load-test-data/20260728-143000 \
  load-test-data/20260728-160000

# Stricter: flag anything >5% regression
bash load-test/reports/compare.sh \
  load-test-data/20260728-143000 \
  load-test-data/20260728-160000 \
  --threshold 5
```

`compare.sh` also prints the RSS delta for each language in both runs — the key proof a memory leak is fixed.  
Exits with code 1 if any regressions found (CI-friendly).

---

## Reference — what the numbers mean

| Observation | Interpretation | Where to dig |
|-------------|---------------|-------------|
| RSS grows linearly at constant VUs | Memory leak | `delta=` in metrics.csv; flamegraph allocations |
| rust-wasm 7–10× slower than rust-embedded | wasmtime#8943 Store-per-message cold start | WASM reinstantiations in `analyze.sh` |
| go/typescript 10–25× slower | Same cold start + language interpreter | Same |
| python 40–50× slower | Componentize-py interpreter on top of WASM cold start | Same |
| All WASM variants equally slow | WASM sandbox cost, not language | PendingAsks contention, mailbox depth |
| p99 >> p95 (long tail) | Lock contention or GC pause | flamegraph: `parking_lot`, `tokio::sync` |
| Error rate > 0 at low VUs | Actor not found or ask timeout | k6 log in `lang-<name>/*.log` |
| Error rate spikes at high VUs | Mailbox overflow | Server log: `lang-<name>/server.log` |
| Latency degrades across all languages | Tokio thread pool saturated | flamegraph: `tokio::runtime` |

---

## Reference — useful one-liners

```bash
# All run IDs collected
ls load-test-data/

# Raw k6 output for a language
cat load-test-data/<run-id>/lang-go/kv_http_go.log

# Server errors for a language
grep -c ERROR load-test-data/<run-id>/lang-python/server.log

# RSS peak for each language in a run
for d in load-test-data/<run-id>/lang-*/; do
  lang=$(basename "$d" | sed 's/lang-//')
  csv="$d/metrics.csv"
  [ -f "$csv" ] || continue
  peak=$(awk -F',' 'NR>1{r=$4+$6; if(r>m)m=r}END{printf "%.0fMB",m}' "$csv")
  printf "%-18s  %s\n" "$lang" "$peak"
done

# Watch RSS in real time (no monitor.sh needed)
watch -n 2 'ps aux | grep -E "perf_actor|plexspaces" | grep -v grep | \
  awk "{rss+=\$6} END {printf \"RSS: %.0f MB\n\", rss/1024}"'
```

---

## Iteration cycle at a glance

```
make fix  →  run.sh --start-server --lang all  →  analyze.sh  →  compare.sh  →  flamegraph  →  repeat
```

Each iteration takes 2–10 minutes (5 languages × 2 min each). The scale ramp (`--scale-levels`) is for finding the ceiling once the baseline is clean.
