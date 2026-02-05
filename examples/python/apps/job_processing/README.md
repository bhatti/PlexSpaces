# Job Processing - Distributed Task Processing (Python WASM with SDK)

Demonstrates **distributed job processing** using TupleSpace for scatter/gather coordination.

**Real-world use cases**: 
- Image processing pipelines (resize, thumbnail generation)
- Data transformation (ETL jobs)
- Batch report generation
- Machine learning inference jobs

## TupleSpace Pattern (Linda-style Coordination)

```
┌─────────────┐     scatter      ┌─────────────────┐
│ Coordinator │ ────────────────► │   TupleSpace    │
│  (submit)   │                   │  (task queue)   │
└─────────────┘                   └────────┬────────┘
                                           │ ts_take
      ┌────────────────────────────────────┼────────────────────────────────────┐
      │                                    │                                    │
      ▼                                    ▼                                    ▼
┌───────────┐                        ┌───────────┐                        ┌───────────┐
│  Worker 1 │                        │  Worker 2 │                        │  Worker N │
│ (process) │                        │ (process) │                        │ (process) │
└─────┬─────┘                        └─────┬─────┘                        └─────┬─────┘
      │                                    │                                    │
      │ ts_write                           │ ts_write                           │ ts_write
      │                                    │                                    │
      └────────────────────────────────────┼────────────────────────────────────┘
                                           │
                                           ▼
                                   ┌─────────────────┐
                                   │   TupleSpace    │
                                   │   (results)     │
                                   └────────┬────────┘
                                           │ ts_read_all
                                           ▼
                                   ┌─────────────┐
                                   │ Coordinator │
                                   │  (gather)   │
                                   └─────────────┘
```

## TupleSpace APIs Used

| API | Usage | Description |
|-----|-------|-------------|
| `ts_write` | Scatter tasks, submit results | Write tuple to coordination space |
| `ts_take` | Claim tasks | Atomic destructive read (prevents duplicate processing) |
| `ts_read` | Check status | Non-destructive read |
| `ts_read_all` | Gather results | Read all matching tuples |

## Tuple Patterns

| Pattern | Fields | Example |
|---------|--------|---------|
| Task | `["job", job_id, "task", task_id, data]` | `["job", "job-1", "task", "task-0", "{\"url\":\"img.jpg\"}"]` |
| Result | `["job", job_id, "result", task_id, data]` | `["job", "job-1", "result", "task-0", "{\"size\":1024}"]` |
| Status | `["job", job_id, "status", value]` | `["job", "job-1", "status", "completed"]` |

## Quick Start

```bash
./build.sh  # Build WASM actor
./test.sh   # Run tests (requires PlexSpaces node)
```

### Start Node

```bash
# Terminal 1: Start node
./scripts/server.sh

# Terminal 2: Run tests
cd examples/python/apps/job_processing
./test.sh 8092
```

## Operations

| Operation | Payload | Description |
|-----------|---------|-------------|
| `submit` | `{"job_type":"resize","tasks":[{"url":"img1.jpg"},{"url":"img2.jpg"}]}` | Submit job with tasks |
| `claim_task` | `{"job_id":"job-1"}` | Claim a task (for workers) |
| `submit_result` | `{"job_id":"job-1","task_id":"task-0","result_data":"..."}` | Submit task result |
| `gather_results` | `{"job_id":"job-1"}` | Gather all results |
| `job_status` | `{"job_id":"job-1"}` | Get job status |
| `list_jobs` | `{}` | List all active jobs |

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Job processor coordinator |
| `state()` | Track job_counter and active_jobs |
| `@handler()` | Route submit, claim_task, gather_results |
| `host.ts_write()` | Write tasks and results |
| `host.ts_take()` | Atomically claim tasks |
| `host.ts_read_all()` | Gather all results |

## Why TupleSpace for Job Processing?

1. **Decoupled**: Workers don't know about coordinator
2. **Atomic**: `ts_take` prevents duplicate processing
3. **Scalable**: Add workers without changing coordinator
4. **Fault-tolerant**: Tasks persist in TupleSpace until processed

## Files

| File | Description |
|------|-------------|
| `job_processing_actor.py` | Job coordinator using TupleSpace |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - TupleSpace API reference
- [MPI Collectives (Rust)](../../rust/embedded/mpi_collectives/) - Rust TupleSpace example
