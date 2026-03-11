# EFlows4HPC native reference: ensemble / workflow orchestration

This file describes how similar HPC ensemble and workflow patterns are implemented in [EFlows4HPC](https://eflows4hpc.eu/) and related HPC workflow systems.

## EFlows4HPC model

- **Workflows**: DAG-based workflows (e.g. Nextflow, Snakemake, or custom) for multi-step HPC jobs.
- **Ensemble runs**: Many independent tasks (Monte Carlo, parameter sweep) executed across the cluster; a coordinator submits jobs and gathers results.
- **Coordination**: Often via shared storage (files), message queues, or coordination services. Linda-style tuple spaces appear in some HPC middleware for task distribution and result collection.
- **Scale**: Fan-out to hundreds or thousands of workers; gather results for aggregation or next stage.

## Native pattern (conceptual)

```python
# Conceptual: Nextflow / Snakemake style
# process run_simulation { ... }
# Channel.from(1..100) | run_simulation | collect
```

- A workflow engine emits a channel of task parameters; each task runs on a worker; results are collected into a channel for the next step.
- In MPI or custom HPC codes: leader scatters work (e.g. via MPI_Scatter or a task queue), workers compute, leader gathers (MPI_Gather or result queue).

## PlexSpaces equivalent

- **Workflow actor**: One coordinator actor per ensemble (virtual_actor); `run(payload)` with `ensemble_id`, `num_tasks`.
- **Scatter**: Write `num_tasks` task tuples to **tuple space** (`["ensemble", id, "task", task_id, param]`).
- **Process**: Loop: `ts_take` one task, simulate work, `ts_write` result tuple (`["ensemble", id, "result", task_id, result]`). With multiple worker actors, each would take tasks and write results; coordinator only gathers.
- **Gather**: `ts_read_all` on result pattern to collect all results.
- **Process groups**: In a multi-worker setup, workers join a group and coordinator can `pg_broadcast` to trigger or coordinate; tuple space remains the task/result channel.

See README comparison table (EFlows4HPC / HPC workflows vs PlexSpaces).
