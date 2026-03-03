# PlexSpaces Examples

Each example’s README should **explain what the example does** and **which PlexSpaces APIs it uses** (e.g. ProcessGroupRegistry, TupleSpace, RequestContext, SqliteKVStore). See `rust/embedded/chat_room`, `rust/embedded/feature_flags`, and `rust/embedded/mpi_collectives` for the expected format (Purpose, What It Demonstrates, API mapping/code patterns, Quick Start).

## Structure

```
examples/
├── rust/
│   ├── embedded/           # 50 Rust embedded examples
│   └── apps/               # 2 Rust WASM actors
├── python/
│   └── apps/               # 10 Python WASM actors
├── typescript/
│   └── apps/               # 2 TypeScript WASM actors
├── go/
│   └── apps/               # 1 Go WASM actor
└── README.md
Native reference code (framework comparison) lives under each example’s `native/` folder when present (e.g. `examples/typescript/apps/migrating_temporal/native/`).

## Summary

| Language | Embedded | Apps | Total |
|----------|----------|------|-------|
| Rust | 50 | 2 | 52 |
| Python | - | 10 | 10 |
| TypeScript | - | 2 | 2 |
| Go | - | 1 | 1 |
| **Total** | **50** | **15** | **65** |

---

## Rust Embedded Examples (50)

### Core Patterns
| Example | Description |
|---------|-------------|
| `actor_groups_sharding` | Data-parallel scaling via sharding |
| `supervision_tree` | Erlang/OTP-style fault tolerance |
| `durable_actor` | Journaling and deterministic replay |
| `timers` | In-memory non-durable timers |
| `reminders` | Durable persistent reminders |
| `process_groups_pubsub` | Pub/Sub coordination |
| `chat_room` | Process groups: join/leave, publish_to_group, list_groups |
| `webhook_handler` | Webhook handler (HTTP deliver/list) |

### Scientific Computing
| Example | Description |
|---------|-------------|
| `heat_diffusion` | 2D stencil computation |
| `matrix_multiply` | Parallel matrix multiplication |
| `matrix_vector_mpi` | MPI-style collective primitives |
| `byzantine` | PBFT consensus |
| `nbody` | Gravitational simulation |
| `nbody_wasm` | N-Body with WASM actors |

### Domain Applications
| Example | Description |
|---------|-------------|
| `genomics_pipeline` | DNA sequence analysis |
| `genomic_workflow_pipeline` | Multi-sample processing |
| `finance_risk` | Monte Carlo risk analysis |
| `order_processing` | Workflow orchestration |
| `timeseries_forecasting` | ML prediction pipeline |
| `entity_recognition` | NER with streaming |

### WASM & Polyglot
| Example | Description |
|---------|-------------|
| `wasm_showcase` | Full WASM capability demo |
| `wasm_calculator` | Python WASM calculator actors |
| `firecracker_multi_tenant` | VM isolation |

### Migration Guides (26)
All `migrating_*` examples show how to migrate from other frameworks.

---

## Running Examples

### Rust Embedded
```bash
cd rust/embedded/<example>
cargo run
```

### Python Apps
```bash
cd python/apps/<app>
componentize-py <actor>.py -o <app>.wasm
plexspaces deploy <app>.wasm --tenant my-tenant
```

### TypeScript Apps
```bash
cd typescript/apps/<app>
jco compile <actor>.ts -o <app>.wasm
plexspaces deploy <app>.wasm --tenant my-tenant
```

### Go Apps
```bash
cd go/apps/<app>
./build.sh    # builds WASM with TinyGo
./test.sh     # deploys and runs E2E tests
```

#### Go SDK Examples

| Example | Description | README |
|---------|-------------|--------|
| **Migrating Erlang/OTP** | Sliding window rate limiter (GenServer pattern) | [README](go/apps/migrating_erlang_otp/README.md) |

---

## TODOs

- [ ] Add more Go apps (counter, calculator)
- [ ] Add more TypeScript apps
- [ ] Review and verify each example compiles/runs
- [ ] Consolidate similar migration examples

See [CHECKLIST.md](CHECKLIST.md) for detailed tracking.
