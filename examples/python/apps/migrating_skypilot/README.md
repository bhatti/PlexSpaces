# Spot Job Runner (Python WASM) – SkyPilot-style

SkyPilot-style **spot job** with **checkpointing** and **failover**: run steps, checkpoint to **KV** after each step; on simulated preemption, next run restores from KV and continues. Uses **Workflow** with **virtual_actor** and **durability**.

## Purpose

- **WorkflowActor**: `run(payload)` with `job_id`, `total_steps`, optional `simulate_preemption_at_step`. Resumes from KV checkpoint if present (failover).
- **KV**: Checkpoint key `skypilot:checkpoint:{job_id}` stores `{current_step, total_steps, timestamp_ms}`; cleared when job completes.
- **Signal**: `workflow_signal:cancel`. **Query**: `workflow_query:status`.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/python/apps/migrating_skypilot
./build.sh
./test.sh 8092
```

## API

- **Run**: `{"op":"workflow_run","job_id":"j1","total_steps":5}` – run to completion or until preemption. Add `"simulate_preemption_at_step":3` to simulate preemption at step 3; call run again (same job_id) to resume from checkpoint.
- **Signal**: `{"op":"workflow_signal:cancel"}`.
- **Query**: `{"op":"workflow_query:status"}`.

## Native (SkyPilot) reference

SkyPilot uses spot VMs and user scripts that checkpoint to object storage; relaunch restores from checkpoint. See **`native/skypilot_ref.md`**. This example uses a single Workflow actor and KV for checkpoints to get the same resume-after-preemption behavior.

## Comparison: SkyPilot vs PlexSpaces

| Feature       | SkyPilot                 | PlexSpaces Python              |
|---------------|--------------------------|---------------------------------|
| Spot          | Cloud spot VMs           | Simulated (preemption_at_step)  |
| Checkpoint    | Object storage (S3/GCS)   | KV (skypilot:checkpoint:job_id) |
| Failover      | Relaunch same task YAML   | Next run() restores from KV     |
| Orchestration | sky launch / spot queue  | WorkflowActor run/signal/query  |

## References

- [PLAN.md – migrating_skypilot](../../../../PLAN.md)
- [SkyPilot](https://github.com/skypilot-org/skypilot)
