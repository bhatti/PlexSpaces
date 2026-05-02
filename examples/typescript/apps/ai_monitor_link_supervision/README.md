# AI Monitor/Link Supervision (TypeScript WASM)

Demonstrates actor-model **monitor** and **link** primitives for building fault-tolerant AI pipelines, using FLP impossibility and Byzantine fault detection as a realistic motivating scenario.

## Overview

This TypeScript WASM example implements an AI pipeline with four actors using the `@plexspaces/sdk`. The `ActorRouter` dispatches to the correct actor class based on the `type` field in `app-config.toml`.

### Key Actors

| Actor | Role | Pattern Used |
|-------|------|-------------|
| `InferenceWorkerActor` | LLM inference backend; normal or Byzantine mode | `host.link()` / `host.unlink()` |
| `ValidatorAgentActor` | Output validator with Byzantine detection (FLP threshold) | `host.monitor()` / `host.demonitor()` |
| `PipelineSupervisorActor` | Fault-aware dispatcher, responds to `__DOWN__` events | `host.monitor()` / `host.demonitor()` |
| `AuditLogActor` | GenEvent fire-and-forget audit log | — |

### Primitives Demonstrated

```typescript
import { host } from "@plexspaces/sdk";

// One-way watch — supervisor continues running when worker stops
const monitorRef = host.monitor?.(workerId);

// Cancel watch — stop receiving __DOWN__ for this actor
host.demonitor?.(monitorRef);

// Bidirectional fate-sharing — abnormal exits only
host.link?.(peerId);

// Safe decoupling before graceful shutdown (normal exits don't cascade)
host.unlink?.(peerId);
```

### Message Handler Pattern

```typescript
// Handle __DOWN__ (monitor fires on ANY termination)
protected "on__DOWN__"(payload: Record<string, unknown>): Record<string, unknown> {
  const monitorRef = String(payload.monitor_ref ?? "");
  const actorId = String(payload.actor_id ?? "");
  this.state.workerPool = this.state.workerPool.filter(w => w !== actorId);
  this.state.monitorRefs = this.state.monitorRefs.filter(m => m.monitorRef !== monitorRef);
  return {};
}

// Handle __EXIT__ (link fires on ABNORMAL exit only)
protected "on__EXIT__"(payload: Record<string, unknown>): Record<string, unknown> {
  const fromActor = String(payload.from_actor ?? "");
  this.state.linkedPeers = this.state.linkedPeers.filter(p => p !== fromActor);
  return {};
}
```

## Usage

```bash
# Build the WASM binary
./build.sh

# Run the full test scenario (requires running PlexSpaces node)
./test.sh                          # default: localhost:8091
./test.sh 8091                     # single node on port 8091
./test.sh localhost:8091 localhost:8094  # two-node cluster
```

## References

- [Architecture](../../../../docs/architecture.md)
- [Detailed Design: Supervision](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Other TypeScript Examples](../../README.md)
