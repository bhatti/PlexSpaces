# Calculator (Rust WASM)

Single GenServer actor: **add**, **subtract**, **multiply**, **divide** on two operands, with **`host::application_metrics_add`** for merged application metrics. Same stack as other `examples/rust/apps/*` SDK+WIT examples (`#[gen_server_actor(wasm)]`, `#[plexspaces_handlers(wasm)]`).

## Layout

| Path | Role |
|------|------|
| `src/lib.rs` | Actor, arithmetic, WIT `Guest` export |
| `app-config.toml` | Supervisor + virtual actor + durability |
| `build.sh` | `wasm32-wasip1` → component `calculator_actor.wasm` |
| `test.sh` | Deploy, ops, batch, `get_status` + `test-common.sh` list |

## Operations

| Handler | Purpose |
|---------|---------|
| `calculate` | `{ "operation": "add"\|"subtract"\|"multiply"\|"divide", "operands": [a, b] }` |
| `reset` | Clear count and last result |
| `get_stats` | `{ calculation_count, last_result }` |
| `get_status` / `status` | Rollups + `use_case` / `orchestration` |
| `batch` | `{ "operations": [ ... same shape as calculate payloads ... ] }` (capped) |

## Metrics

- Per successful `calculate`: `calculator_ops`, `calculator.compute` / `calculator.coordination` latencies.
- `reset`: `calculator_resets`.
- `batch`: extra `calculator_batch_runs`, `calculator_batch_ops_delta`, `calculator_batch_errors_delta`.

## Build & test

```bash
cd examples/rust/apps/calculator
./build.sh
./scripts/server.sh   # repo root, separate terminal
./test.sh
```

## References

- [Architecture](../../../../docs/architecture.md)
- [SDK](../../../../docs/sdk.md)
- [Apps checklist](../SDK_WIT_APPS_CHECKLIST.md)
