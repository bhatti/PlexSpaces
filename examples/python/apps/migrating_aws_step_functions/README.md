# ML Pipeline Workflow (Python WASM) – AWS Step Functions style

AI/ML pipeline workflow: **data_prep (fan-out)** → **training** → **evaluation** → **deploy**. Uses **@workflow_actor** with virtual_actor and durability (same pattern as migrating_temporal).

## Purpose

- **@workflow_actor(facets=["virtual_actor", "durability"])**: Workflow with facets; app-config supplies virtual_actor + durability.
- **Pipeline steps**: Data prep (fan-out to N shards, fan-in) → Training → Evaluation → Deploy; cancel signal for compensation.
- **Virtual actor**: One workflow instance per pipeline run (`ml-pipeline:pipe-1`, etc.) via virtual_actor facet.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2 (with venv that has plexspaces-py)
cd examples/python/apps/migrating_aws_step_functions
./build.sh
./test.sh 8091
```

## API

- **Run**: `POST /api/v1/actors/{app_id}/ml-pipeline:{pipeline_id}` with `{"op":"workflow_run","pipeline_id":"..."}`.
- **Signal**: Same path with `{"op":"workflow_signal:cancel"}`.
- **Query**: Same path with `{"op":"workflow_query:status"}`.

## Comparison

| Feature       | AWS Step Functions     | PlexSpaces                    |
|---------------|------------------------|-------------------------------|
| Workflow      | State machine / states | @workflow_actor run/signal/query |
| Parallel      | Map state              | Fan-out in run() (e.g. N shards) |
| Durability    | Built-in               | Durability facet              |
| Idle timeout  | N/A                    | virtual_actor idle_timeout   |

## References

- [PLAN.md – migrating_aws_step_functions](../../../../PLAN.md)
- [AWS Step Functions](https://docs.aws.amazon.com/step-functions/)
