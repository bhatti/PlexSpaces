# Genomics Pipeline (Rust WASM App)

**Single-actor only.** This example does **not** implement leader-worker or multi-node distribution. One WASM actor runs the full pipeline (QC → Alignment → Variant Calling) on the node that receives the request.

## Scope: What runs where

| Role   | Code / behavior |
|--------|------------------|
| Leader | **N/A** – there is no leader. |
| Worker | **N/A** – there are no workers. |
| This actor | Single pipeline actor. Handles `run`, `query`, `signal`. Runs QC → Alignment → Variant Calling in one process on the entry node only. |

With multiple nodes, the test script deploys the same WASM app to every node and sends **one** `run` to the **entry node**. That run is executed entirely by this single actor on the entry node. Other nodes are not used for this pipeline run.

## Real-world use case

DNA sequencing analysis workflow (GATK/Illumina-style): one sample, one process. For distributed runs (leader splitting work across workers), a different example or app would implement that.

## Quick start

**1. Start the server** (from repo root):

```bash
./scripts/server.sh
```

**2. Build and test** (from this directory):

```bash
cd examples/rust/apps/genomics_pipeline
./build.sh
./test.sh 8092
```

- `build.sh`: Builds for `wasm32-wasip1`, produces `genomics_actor.wasm`. Uses shared workspace target.
- `test.sh [HTTP_PORT | host:port host:port ...]`: Deploys to each node, sends **one** run to the **entry node** (first in list). That run is executed by the **single** pipeline actor on the entry node only.

## What this example demonstrates

1. **WASM app** – Implements `plexspaces:simple-actor` ABI (`init`, `handle`, `get-state`, `set-state`); GenServer-style ops: `run`, `query`, `signal`.
2. **Single pipeline** – One actor runs QC → Alignment → Variant Calling synchronously.
3. **Benchmarks** – Data size (input reads), compute vs coord time, efficiency, granularity. Uses 400k reads by default so the run is non-trivial and metrics are visible.
4. **Observability** – Framework logs `WasmActor invoked` with `actor_id`, `op`, `payload_len`. Response includes `single_actor: true` and `data_size_reads`.

## Architecture

- **run**: Payload is `PipelineInput` (sample_id, num_reads, reference_genome, min_quality_score). Actor runs the three steps in WASM and returns `PipelineState` with results and compute/coordinate times. Response includes `single_actor: true` and `data_size_reads`.
- **query**: Payload specifies `query` (`status`, `progress`, `metrics`). Metrics include `single_actor`, `data_size_reads`.
- **signal**: Payload specifies `signal` (`pause`, `resume`, `cancel`).

## Data size and benchmarks

Default `num_reads` is 400k so the run takes a few seconds and metrics are meaningful. `test.sh` prints data size, compute/coord times, efficiency, and states clearly that the pipeline is single-actor on the entry node. Per-node actor counts (when multiple nodes are used) are printed from the nodes API.

## Config

`app-config.toml` defines one GenServer child, `pipeline`, with optional virtual_actor facet. The server loads the WASM and routes `run` / `query` / `signal` to this actor.

## Multi-node: what the test does (no distribution)

When you pass multiple host:port (e.g. `./test.sh localhost:8092 localhost:8094`):

1. **ConnectNodes**: Entry node connects to the other node(s) so the cluster is formed.
2. **Deploy**: The same WASM app is deployed to **all** nodes (so each node could run this actor type).
3. **One run**: The client sends **one** `run` to the **entry node** only. The **single** pipeline actor on the entry node executes the full run. No work is sent to other nodes.
4. **Metrics**: The script may query each node’s API for actor counts and prints a clear line that execution was single-actor on the entry node.

This example does **not** implement a leader that splits work to workers on other nodes. For that pattern, see the leader-worker SDK and multi-node examples that use it.

## See also

- [migrating_skypilot](../../rust/apps/migrating_skypilot) – Same WASM app pattern
- [Examples README](../../../README.md) – Multi-node and leader-worker design
- [Architecture](../../../../docs/architecture.md)
