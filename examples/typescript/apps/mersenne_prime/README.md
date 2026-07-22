# Mersenne Prime Distributed Computation (TypeScript WASM + WsThinClient)

A SETI@home-style distributed computation where browser tabs connect as
**thin-node workers**, each running a Lucas-Lehmer primality test in a
**Web Worker Blob URL** (no eval, no external scripts), driven by a WASM
`CoordinatorActor` that manages the work queue and collects results in
TupleSpace.

Demonstrates resource-aware thin-node scheduling: the coordinator assigns
small exponents to low-core nodes and large exponents to high-core nodes
using the `cpu_cores` capability reported in the `NodeRegistration` frame.

## What It Demonstrates

| PlexSpaces Feature | Role |
|---|---|
| `WsThinClient` | Zero-dep thin-node SDK; browser tabs are workers |
| `NodeRegistration.capabilities` | `cpu_cores` hint for resource-aware scheduling |
| `PingResponse` resource fields | `cpu_percent`, `memory_available_mb`, `available_cores` (proto fields 9–11) |
| `PlexSpacesActor<State>` | Typed actor base class; state survives actor restarts |
| `ActorRouter` | Dispatch to `CoordinatorActor` and `CodeServerActor` |
| `host.send()` | Server → thin-node work assignment via WsActorTransportClient |
| `host.ts.write()` | Result aggregation in TupleSpace |
| `host.incrCounter()` | Prometheus-compatible per-app metric counters |
| Web Worker Blob URL | Lucas-Lehmer in a real thread — no eval, no CDN script |

## Actors

| Class | Behavior | Facets | Role |
|---|---|---|---|
| `CoordinatorActor` | GenServer | `virtual_actor` | Work queue, assignment, result collection, status |
| `CodeServerActor` | GenServer | `virtual_actor` | Serves Lucas-Lehmer Web Worker JS string |

## Architecture

```
Browser Tab (WsThinClient, ULID node_id)
  │  1. ask CodeServerActor "get_worker_js"
  │  2. spawn Worker(Blob([code]))
  │  3. tell CoordinatorActor "ready" {actor_id, cpu_cores}
  ▼
CoordinatorActor:{mersenne}  (WASM TypeScript)
  │  host.send(actor_id, "assign_work", {p})
  │  via WsActorTransportClient → thin-node session
  ▼
Browser Tab — Web Worker receives {p}, computes lucasLehmer(BigInt(p))
  │  tell CoordinatorActor "result" {p, is_prime, duration_ms, actor_id}
  ▼
CoordinatorActor  →  host.ts.write(["result", p, is_prime, …])
                  →  host.incrCounter("ts-mersenne-prime", "primes_found")
                  →  assign next work to worker
```

## Lucas-Lehmer Test

```javascript
// Runs inside a Web Worker Blob URL — pure BigInt, no eval
function lucasLehmer(p) {
  if (p === 2n) return true;
  const mp = (1n << p) - 1n;       // Mersenne number: 2^p - 1
  let s = 4n;
  for (let i = 0n; i < p - 2n; i++) s = (s * s - 2n) % mp;
  return s === 0n;                  // Prime iff remainder is 0
}
```

Candidates: `[2, 3, 5, 7, 13, 17, 19, 31, 61, 89, 107, 127, 521, 607, 1279, 2203, 2281, 3217, 4253, 4423]`
— known Mersenne prime exponents up to M4423.

## Scaling Benchmark

`test.sh` runs the same 8 candidates twice (1 worker, then 2 workers) and
prints a parallel speedup table:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│              mersenne_prime  Distributed Scaling Benchmark                  │
│  (Lucas-Lehmer BigInt, candidates 2–31, browser workers via WsThinClient)   │
├─────────┬────────────┬──────────────┬───────────┬──────────────┬────────────┤
│ Workers │ Candidates │ Found Primes │ Elapsed s │  Throughput  │  Speedup   │
├─────────┼────────────┼──────────────┼───────────┼──────────────┼────────────┤
│       1 │          8 │            8 │      1.20 │       6.67/s │  1.00x (100%) │
│       2 │          8 │            8 │      0.65 │      12.31/s │  1.85x ( 92%) │
└─────────┴────────────┴──────────────┴───────────┴──────────────┴────────────┘

Routing path: WsThinClient → WS endpoint → CoordinatorActor.onReady()
              → host.send(worker_actor_id, 'assign_work') → thin-node session
              → lucasLehmer(BigInt) → host.send(coord, 'result')
              → host.ts.write(['result', ...]) + host.incrCounter()
```

*(Numbers measured on a local single-node PlexSpaces instance.)*

## Browser UI

```
┌─────────────────────────────────────────────────────────────┐
│  ⚛ PlexSpaces Mersenne Prime Finder                         │
│  URL: [ws://localhost:8091/ws]  [Connect & Start]           │
│  Cores: 8  Node: 01JXXXXXXXXXX                             │
├─────────────────────────────────────────────────────────────┤
│  Progress: ████████░░░░░░░░  6/8  Workers: 1               │
│  [2^2-1=3 ✓][2^3-1=7 ✓][2^5-1=31 ✓][2^7-1=127 ✓]         │
│  [2^13-1=8191 ✓][2^17-1 working…][2^19-1 pending][2^31-1…] │
├─────────────────────────────────────────────────────────────┤
│  10:05:01 Assigned: test 2^17-1                            │
│  10:05:00 2^13-1=8191 is PRIME!                            │
└─────────────────────────────────────────────────────────────┘
```

## Build & Run

Prerequisites: PlexSpaces server running on port 8091.

```bash
cd examples/typescript/apps/mersenne_prime

# Run automated integration test (1-worker + 2-worker scaling benchmark),
# then keep app deployed for browser use:
bash test.sh
```

After `test.sh` completes, open the browser UI at:

**`http://localhost:8091/apps/ts-mersenne-prime/`**

Connect with one or more browser tabs — each tab becomes an independent compute
worker, fetches the Lucas-Lehmer JS from `CodeServerActor`, and processes work
assignments from `CoordinatorActor`.

> Do NOT open `static/index.html` directly from the filesystem — the browser
> blocks WebSocket connections from `file://` origins. Always use the HTTP URL above.

The static files are bundled into the deploy zip (WAR-style) alongside `app.wasm`
and `app-config.toml`. The node serves them immediately at deploy time — no
server restart needed.

To undeploy after you're done:
```bash
bash undeploy.sh
```

To use a different port or more candidates:
```bash
WS_PORT=8093 bash test.sh
CANDIDATE_COUNT=20 bash test.sh
```

To force undeploy at the end of the test:
```bash
KEEP_DEPLOYED=0 bash test.sh
```

## Configuration

| Variable | Default | Description |
|---|---|---|
| `WS_PORT` | `8091` | PlexSpaces node WS port |
| `LEADER_NODE_ID` | `test-node-8091` | Node ID where actors are deployed |
| `TIMEOUT` | `120` | Test timeout in seconds |
| `CANDIDATE_COUNT` | `8` | Number of candidates (max 20) |
| `KEEP_DEPLOYED` | `1` | Set to `0` to undeploy after test completes |

## References

- [Architecture — WebSocket thin nodes](../../../../docs/architecture.md)
- [Getting Started](../../../../docs/getting-started.md)
- [TypeScript SDK README](../../../../sdks/typescript/README.md)
- [web_crawl example](../web_crawl/README.md) — parallel compute via ShardGroup / ScatterGather
