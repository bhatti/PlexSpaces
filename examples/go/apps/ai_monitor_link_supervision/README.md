# AI Monitor/Link Supervision (Go WASM)

Demonstrates actor-model **monitor** and **link** primitives for building fault-tolerant AI pipelines, using FLP impossibility and Byzantine fault detection as a realistic motivating scenario.

## Overview

In distributed AI systems, inference workers can fail silently (crash), return inconsistent outputs (Byzantine mode), or become unreachable (network partition). This example shows how PlexSpaces monitor/link primitives — inspired by Erlang/OTP and Akka Death Watch — let supervisors detect these failures and respond appropriately.

### Key Actors

| Actor | Role | Pattern Used |
|-------|------|-------------|
| `InferenceWorker` | LLM inference backend; can be in normal or Byzantine mode | `host.Link()` / `host.Unlink()` |
| `ValidatorAgent` | Validates outputs; detects Byzantine behavior; monitors workers | `host.Monitor()` / `host.Demonitor()` |
| `PipelineSupervisor` | Routes requests; monitors workers; responds to `__DOWN__` events | `host.Monitor()` / `host.Demonitor()` |
| `AuditLogActor` | GenEvent-style fire-and-forget event log | (observer, no monitor) |

### Primitives Demonstrated

```go
// One-way watch — supervisor continues running when worker stops
monitorRef := host.Monitor(workerActorId)

// Cancel watch — e.g., before replacing a worker
host.Demonitor(monitorRef)

// Bidirectional fate-sharing — abnormal exit propagates to peer
host.Link(peerActorId)

// Safe decoupling — unlink before graceful shutdown to prevent cascade
host.Unlink(peerActorId)
```

### Message Handlers

| Message Type | Actor | Description |
|-------------|-------|-------------|
| `__DOWN__` | `ValidatorAgent`, `PipelineSupervisor` | Worker stopped (any reason) — Monitor fires |
| `__EXIT__` | `InferenceWorker` | Linked peer died abnormally — Link fires |
| `infer` | `InferenceWorker` | Run inference (normal or Byzantine mode) |
| `validate` | `ValidatorAgent` | Validate result; FLP-inspired Byzantine detection |
| `monitor_worker` | `ValidatorAgent`, `PipelineSupervisor` | Attach monitor to a worker |
| `dispatch` | `PipelineSupervisor` | Route inference request to available worker |
| `link_with` | `InferenceWorker` | Establish bidirectional link with peer |
| `unlink_from` | `InferenceWorker` | Remove link before graceful shutdown |
| `set_mode` | `InferenceWorker` | Switch between `normal` and `byzantine` modes |
| `status` | All actors | Return actor state and counters |

## FLP Impossibility Context

The [FLP theorem](https://groups.csail.mit.edu/tds/papers/Lynch/jacm85.pdf) (Fischer, Lynch, Paterson 1985) proves that in an **asynchronous** distributed system, no deterministic protocol can guarantee consensus if even one process may crash.

**Actor model response**: Don't try to solve FLP. Instead:
1. **Monitors** provide crash detection — supervisor receives `__DOWN__` on any termination
2. **Links** provide fate-sharing — abnormal exits cascade to linked peers
3. **Unlink before graceful shutdown** — prevents normal restarts from looking like crashes

## Byzantine Fault Context

A Byzantine actor returns **inconsistent, adversarial, or corrupted** outputs without crashing. Classical result: tolerating `f` Byzantine faults requires `3f+1` replicas.

**Practical AI equivalent**: A model under adversarial prompting or serving stale checkpoints can return syntactically valid but semantically wrong outputs. The `ValidatorAgent` demonstrates threshold-based detection: when ≥1/3 of validations fail, it flags the worker as potentially Byzantine.

## Usage

```bash
# Build the WASM binary
./build.sh

# Run the full test scenario (requires running PlexSpaces node)
./test.sh                          # default: localhost:8091
./test.sh 8091                     # single node on port 8091
./test.sh localhost:8091 localhost:8094  # two-node cluster
```

## Test Scenario

The test script exercises 5 phases:

1. **Normal operation** — Both workers handle inference requests
2. **Monitor setup** — Supervisor and validator attach monitors via `host.Monitor()`
3. **Link setup** — Workers linked bidirectionally via `host.Link()`
4. **Byzantine injection** — Worker-a switched to Byzantine mode; validator detects inconsistency
5. **Recovery** — `host.Unlink()` decouples workers; worker-a reset to normal mode

## What This Demonstrates

- **Monitor semantics**: `__DOWN__` is delivered for any exit (normal, error, or kill). The monitoring actor keeps running — it's a notification, not fate-sharing.
- **Link semantics**: `__EXIT__` is delivered only on **abnormal** exits. Normal and `Shutdown` exits do NOT propagate. This is Akka Death Watch / OTP `trap_exit` semantics.
- **Unlink before shutdown**: Pattern for graceful restart without cascade failures.
- **Byzantine detection threshold**: Practical approximation of the 1/3 Byzantine bound for AI output validation.

## References

- [Architecture](../../../../docs/architecture.md)
- [Detailed Design: Supervision](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Other Go Examples](../../README.md)
