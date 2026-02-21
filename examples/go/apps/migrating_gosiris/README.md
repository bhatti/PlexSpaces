# Migrating from Gosiris: IoT Sensor Aggregation

**Use Case**: Industrial IoT monitoring — temperature/humidity sensors across data center zones reporting to an aggregation actor that computes rolling statistics and detects anomalies.

## PlexSpaces Abstractions Used

| Abstraction | Usage |
|---|---|
| **GenServer** | Both SensorActor and AggregatorActor use request-reply patterns |
| **Process Groups** | Sensors auto-register in groups; aggregator discovers via `host.PG().Members()` |
| **ActorRouter** | Single WASM module hosts both sensor and aggregator actors |
| **State Persistence** | `GetState/SetState` JSON serialization with BaseActor |

## Architecture

```
  sensor-dc-zone-a ──┐
  sensor-dc-zone-b ──┤── "sensors" process group
  sensor-server-room ┤
  sensor-outdoor ────┘
                      │
                      ▼
               aggregator
            (polls via PG.Members → Ask)
            (computes rolling stats)
            (detects anomalies)
```

## Gosiris vs PlexSpaces Comparison

| Gosiris | PlexSpaces Go WASM |
|---|---|
| `gosiris.Actor` interface | `plexspaces.Actor` interface |
| `actor.Receive(ctx, msg)` | `actor.Handle(from, msgType, payload)` |
| `system.ActorOf(name, actor)` | `app-config.toml` supervisor children |
| `ctx.Send(ref, msg)` | `host.Send(actorID, msgType, payload)` |
| Manual actor registry | `ActorRouter` prefix-based routing |
| In-process only | Distributed WASM nodes |
| No supervision | OTP-style supervisor restart strategies |
| No process groups | Built-in `host.PG().Join/Members/Broadcast` |

## Quick Start

```bash
# Start PlexSpaces node
PLEXSPACES_JWT_SECRET=test cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:7992

# Build and test
./build.sh
./test.sh
```

## Files

| File | Purpose |
|---|---|
| `sensor_aggregation.go` | SensorActor + AggregatorActor (Go WASM) |
| `app-config.toml` | Supervisor with 4 sensors + 1 aggregator |
| `build.sh` | TinyGo WASM build |
| `test.sh` | Deploy + integration test with benchmarks |
| `native/sensor_system.go` | Gosiris-style reference implementation |
