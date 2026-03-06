# HPC Ensemble (Rust) – EFlows4HPC-style

Same behavior and config as the [Python](../../../python/apps/migrating_eflows4hpc/README.md), [TypeScript](../../../typescript/apps/migrating_eflows4hpc/README.md), and [Go](../../../go/apps/migrating_eflows4hpc/README.md) examples: **single actor type** with coordinator and workers using **tuple space** and **process group**.

## Status

Rust WASM actor for this example would use the same WIT host interface (`ts_write`, `ts_take`, `ts_read_all`, `pg_join`, `pg_broadcast`) as the other SDKs. The **app-config.toml** and test flow are identical; you can deploy the Python-, TypeScript-, or Go-built WASM to verify the node and config.

## Abstractions (same as other languages)

- **Tuple space**: list-in/list-out; write task tuples, take/read result tuples; use wildcards in patterns.
- **Process group**: workers join on first use; coordinator broadcasts `tasks_ready` with data-only payload; msgType is used for routing.

## Quick Start

Build and run tests using the Python example’s WASM (build Python example first):

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2 – build Python WASM once
cd examples/python/apps/migrating_eflows4hpc && ./build.sh && cd -

# Run test from Rust example dir (uses Python WASM)
cd examples/rust/apps/migrating_eflows4hpc
cp ../../../python/apps/migrating_eflows4hpc/ensemble_actor.wasm .  # optional: use Python WASM
./test.sh 8092
```

Or use the TypeScript/Go-built WASM: copy `ensemble_actor.wasm` from the respective example into this dir, then `./test.sh 8092`.

## References

- [Python EFlows4HPC example](../../../python/apps/migrating_eflows4hpc/README.md)
- [TypeScript EFlows4HPC example](../../../typescript/apps/migrating_eflows4hpc/README.md)
- [Go EFlows4HPC example](../../../go/apps/migrating_eflows4hpc/README.md)
- [docs/sdk.md – Cross-SDK consistency](../../../../docs/sdk.md)
