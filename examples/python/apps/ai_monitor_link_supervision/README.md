# AI Monitor/Link Supervision (Python WASM)

Demonstrates actor-model **monitor** and **link** primitives for building fault-tolerant AI pipelines, using FLP impossibility and Byzantine fault detection as a realistic motivating scenario.

## Overview

In distributed AI systems, inference workers can fail silently (crash), return inconsistent outputs (Byzantine mode), or become unreachable (network partition). This example shows how PlexSpaces monitor/link primitives let supervisors detect these failures and respond appropriately.

### Key Actors

| Actor | Role | Pattern Used |
|-------|------|-------------|
| `InferenceWorker` | LLM inference backend; normal or Byzantine mode | `host.link()` / `host.unlink()` |
| `ValidatorAgent` | Output validator with Byzantine detection (FLP threshold) | `host.monitor()` / `host.demonitor()` |
| `PipelineSupervisor` | Fault-aware dispatcher responding to `__DOWN__` events | `host.monitor()` / `host.demonitor()` |
| `AuditLogActor` | GenEvent fire-and-forget audit log | (observer) |

### Primitives Demonstrated

```python
from plexspaces import host

# One-way watch — supervisor continues running when worker stops
monitor_ref = host.monitor(worker_actor_id)

# Cancel watch — e.g., before replacing a worker
host.demonitor(monitor_ref)

# Bidirectional fate-sharing — abnormal exit propagates to peer
host.link(peer_actor_id)

# Safe decoupling — unlink before graceful shutdown
host.unlink(peer_actor_id)
```

### Message Handling

```python
@handler("__DOWN__", "cast")
def on_down(self, monitor_ref: str = "", actor_id: str = "", reason: str = "") -> None:
    # Worker stopped — monitor fires for ANY exit (normal or error)
    self.worker_pool.remove(actor_id)

@handler("__EXIT__", "cast")
def on_exit(self, from_actor: str = "", reason: str = "") -> None:
    # Linked peer died abnormally — only fires on ERROR exits
    # Normal exits and Shutdown do NOT propagate
    self.linked_peers.remove(from_actor)
```

## FLP Impossibility Context

The FLP theorem proves that in an async distributed system, no deterministic protocol can guarantee consensus if even one process may crash. The actor model responds with:

1. **Monitors** — crash detection via `__DOWN__` messages
2. **Links** — fate-sharing via `__EXIT__` messages
3. **Unlink before shutdown** — prevents normal restarts from cascading

## Byzantine Fault Detection

The `ValidatorAgent` applies a threshold-based detection heuristic: when ≥1/3 of all validation checks fail, it raises a Byzantine alert. This approximates the classical 3f+1 Byzantine fault tolerance bound.

## Usage

```bash
# Build the WASM binary
./build.sh

# Run the full test scenario (requires running PlexSpaces node)
./test.sh                          # default: localhost:8092
./test.sh 8092                     # single node on port 8092
./test.sh localhost:8092 localhost:8094  # two-node cluster
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Detailed Design: Supervision](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Other Python Examples](../../README.md)
