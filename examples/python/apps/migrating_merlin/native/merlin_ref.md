# Merlin / HPC parameter sweep reference

This file describes how similar **parameter sweep** and **scientific simulation** patterns are implemented in [Merlin](https://github.com/LLNL/merlin) and related HPC workflow tools.

## Merlin (LLNL)

- **Purpose**: Parameter study workflow for HPC. Define a parameter space, run many simulations (possibly with adaptive sampling), aggregate results.
- **Concepts**: Workflow (DAG or step-based), worker pool (distributed workers pull work), queue (task queue), aggregation.
- **Typical flow**: Coordinator generates parameter sets → workers pull tasks from a queue → run simulation → push results → coordinator aggregates.

## PlexSpaces mapping

| Merlin / HPC                 | PlexSpaces (this example)                          |
|-----------------------------|----------------------------------------------------|
| Worker pool                 | Process group `merlin-workers` (workers join)     |
| Task queue                 | Tuple space (`host.ts`): tasks written, workers take |
| Parameter set → worker     | Coordinator `host.ts.write` task tuples; broadcast `work_available` |
| Worker runs simulation     | Worker `host.ts.take` → compute → `host.ts.write` result |
| Aggregate                  | Coordinator `host.ts.read_all` result pattern      |
| Elastic pool size          | Number of worker instances that join the group (wake more = scale out) |

## References

- [Merlin (LLNL)](https://github.com/LLNL/merlin)
- [Python SDK – host.ts and process groups](../../../../sdks/python/README.md)
