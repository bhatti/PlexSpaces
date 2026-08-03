# PlexSpaces Load Test Plan

## Why This Exists

Performance problems almost never appear in unit tests. They show up when many users hit the system simultaneously — and even then, only some problems surface immediately. Others take 30 minutes of sustained load before a slow memory leak fills the heap.

This test plan exists to answer four questions we cannot answer any other way:

1. What is the real per-actor memory overhead? Erlang advertises ~300 bytes per lightweight process. What is our number?
2. How much does the WASM sandbox cost compared to a native embedded actor? That gap directly affects what workloads are practical to run on this framework.
3. Where does the system break, and how does it break? Does it reject requests cleanly, or does it fall over?
4. Which code path is the bottleneck? The WASM dispatch, the Tokio thread pool, the SQLite journal, or something else?

---

## Goals

| Goal | How we measure it |
|------|------------------|
| Establish latency baseline for every language+transport combination | p50/p95/p99 from k6 echo scenario |
| Find WASM overhead vs embedded (native) actor | Compare rust-embedded vs rust-wasm echo latency |
| Find overhead per actor (memory) | actor_capacity scenario: spawn until OOM, divide |
| Find maximum sustainable throughput per language | Step load from 100 → 10,000 VUs until error rate > 5% |
| Verify actor groups work at scale | ShardGroup 500/1000/2000 shards, PG 500/1000/2000 members |
| Identify top 3 bottlenecks from flamegraphs | CPU + heap flamegraph during peak load |

---

## What We Test

### Core Operations (5 per language)

| Operation | What it does | Why it matters |
|-----------|-------------|----------------|
| `echo` | Return payload unchanged | Measures pure routing + WASM sandbox overhead. Zero compute, so any latency here is pure framework cost. |
| `compute` | Lucas-Lehmer Mersenne prime check (~1ms CPU) | Adds realistic compute cost. Lets us separate "how fast can we route messages" from "how fast can we do work". |
| `kv_put` + `kv_get` | Write + read a key via the KV host function | Tests the host-function bridge overhead and SQLite write/read latency. A slow KV is a signal to look at disk I/O or SQLite configuration. |
| `pg_broadcast` | Join a process group and broadcast one message | Tests the pub/sub path. If this is slow with 2000 members, the fan-out implementation needs work. |
| `shard_task` | Accept a data slice, run one gradient descent step | Realistic HPC workload. Used by the ShardGroup tests. |

### Languages Tested

- **Python WASM** — compiled via componentize-py
- **Go (TinyGo) WASM** — compiled via TinyGo targeting wasi
- **TypeScript WASM** — compiled via jco componentize
- **Rust WASM** — compiled via cargo + wasm-tools
- **Rust Embedded** — native Tokio actor, no WASM sandbox. This is the baseline.

### Transports Tested

- **HTTP/REST**: `POST /api/v1/actors/:app/:type/:name/ask`
- **gRPC**: `ActorRuntimeService.AskReply`

Note: some capabilities (like pg_broadcast via WIT) are in-process-only and do not have a direct external gRPC equivalent. Those tests use the ProcessGroupService gRPC API at the cluster level instead.

### Actor Groups

| Group type | Test sizes | Workload |
|-----------|-----------|---------|
| ShardGroup | 500 / 1000 / 2000 shards | Scatter-gather → all-reduce → barrier (gradient descent) |
| ProcessGroup | 500 / 1000 / 2000 members | Broadcast 100 messages, verify delivery |
| ElasticPool | min=10, max=2000 | Concurrent checkout → compute → checkin |

---

## Test Types Included

### Load Test
Runs expected concurrent load (100 VUs) to verify the system handles normal traffic within latency targets. This is the sanity check we run on every PR that touches a hot path.

### Stress Test
Pushes beyond expected load (500 → 1000 → 2000 → 5000 → 10,000 VUs) to find where the system breaks and how it breaks. We stop when error rate exceeds 5% for 30 seconds or p99 exceeds 30 seconds. The goal is to know our limits before production reveals them.

### Spike Test
Jump from 0 to 2000 VUs without a ramp. Tests whether the system can absorb a sudden traffic spike (a newsletter going out, a batch job firing). We watch: does latency return to baseline after the spike, or does it stay elevated (meaning the system is carrying pressure forward)?

### Soak Test
Run 500 VUs continuously for 30 minutes. This catches memory leaks, connection exhaustion, and GC pressure that only appear after the system has been running a while. A system that is fine for 5 minutes but exhausted after 30 minutes has a slow leak somewhere.

### Capacity Test
Spawn actors in batches until the node runs out of memory. Measures overhead per actor and maximum actor count at 1G and 2G memory limits.

---

## Acceptance Criteria

These are the initial targets. They will be revised upward as we fix bottlenecks.

| Test | Metric | Target |
|------|--------|--------|
| echo HTTP, 100 VUs, rust-embedded | p99 | < 100ms |
| echo gRPC, 100 VUs, rust-embedded | p99 | < 50ms |
| echo HTTP, 100 VUs, python WASM | p99 | < 500ms |
| echo gRPC, 100 VUs, python WASM | p99 | < 300ms |
| kv put+get HTTP, 100 VUs | p99 | < 500ms |
| ShardGroup 500 shards scatter-gather | total time | < 10s |
| Actor capacity, 1G limit | max actors | > 10,000 |
| Actor overhead | bytes/actor | < 50KB |
| Error rate, any test | rate | < 0.1% at 100 VUs |

---

## What We Do NOT Test

- External databases (all tests use in-process SQLite)
- Cloud APIs or network-dependent services
- Multi-node cluster behavior (single local node)
- Authentication overhead (auth disabled for load testing)
- WASM module compile time (actors are pre-deployed, not cold-compiled during the test)

These exclusions keep the tests focused and fast. Add them back one at a time as the core numbers look good.

---

## How to Read the Results

**If gRPC is slower than HTTP**: The gRPC handler path has a bottleneck. Look at the flamegraph for the tonic handler thread.

**If Python and Go WASM are dramatically slower than Rust WASM**: The WASM runtime dispatch is not the bottleneck — the language runtime itself is (Python interpreter overhead, Go GC).

**If all WASM languages are slow by the same factor vs embedded**: The WASM sandbox itself is the bottleneck. Look at host-function call overhead and module instantiation time.

**If latency grows linearly with shard count in ShardGroup**: Normal — more shards means more work. If it grows faster than linearly, the aggregation step is the bottleneck.

**If actor capacity is much lower than 10,000 at 1G**: Actor struct size, Tokio task stack, or mailbox buffer is too large. The flamegraph heap capture will point to the allocation site.

---

## Repeat Cadence

| When | What to run |
|------|-------------|
| Before each PR that touches actor routing, WASM runtime, or Tokio scheduler | `--mode echo --lang all --transport both --vus 100 --iters 100` |
| Weekly (automated) | Full suite at 500 VUs |
| Before each major release | Full suite at all load levels including soak test |
| After a runtime/dependency upgrade | Full suite to catch allocation pattern changes |

---

## How to Add a New Test

1. Add the operation to all 5 `perf_actor` implementations (same name, same behavior).
2. Add a k6 scenario file in `load-test/k6/scenarios/`.
3. Add the new `--mode` to `run.sh`.
4. Add acceptance criteria to this document.
5. Run it at 1 VU first to verify correctness, then at 100 VUs for baseline.
