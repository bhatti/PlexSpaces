# GPU Job Scheduler (Python WASM) – Kueue-style

Kueue-style **GPU job queue**: priority queue, resource quotas (GPUs), **allocate** under a **lock**, **preemption** (requeue). Uses **GenServer** with **virtual_actor** (on-demand activation) and **locks** facet; **KV** for job metadata; host lock for allocate critical section.

## Purpose

- **GenServer**: Handlers `submit`, `allocate`, `complete`, `preempt`, `list_queue`, `get_quotas`.
- **Lock**: Allocate acquires `host.lock_acquire("kueue-allocator")` before picking a job and releasing quota; release when done.
- **KV**: Job metadata stored at `kueue:job:{job_id}` on submit (optional durability).
- **Priority**: Pending jobs sorted by priority (desc), then submitted_at (asc); allocate returns first that fits quota.

## Quick Start

```bash
# Terminal 1 (from repo root)
./scripts/server.sh

# Terminal 2
cd examples/python/apps/migrating_kueue
./build.sh
./test.sh 8092
```

## API

- **submit**: `{"op":"submit","job_id":"j1","priority":10,"gpu_request":2}` – add job to queue.
- **allocate**: `{"op":"allocate"}` – allocate next job by priority (under lock); returns `job_id` or no capacity.
- **complete**: `{"op":"complete","job_id":"j1"}` – free GPUs, mark completed.
- **preempt**: `{"op":"preempt","job_id":"j1"}` – free GPUs, re-queue job as pending.
- **list_queue**: `{"op":"list_queue"}` – pending and allocated lists.
- **get_quotas**: `{"op":"get_quotas"}` – current `used_gpus`, `peak_used_gpus`, max_gpus, processed_count, metrics.

## Native (Kueue) reference

Kueue uses Kubernetes ClusterQueue/LocalQueue and Job priority/quota. See **`native/kueue_ref.md`** for YAML sketch and quota/preemption. This example provides the PlexSpaces equivalent with a single GenServer + lock + KV.

## Comparison: Kueue vs PlexSpaces

| Feature      | Kueue                    | PlexSpaces Python              |
|-------------|---------------------------|---------------------------------|
| Queue       | ClusterQueue + LocalQueue | In-memory queue (state)         |
| Quota       | nominalQuota (resources)   | max_gpus, used_gpus, peak_used_gpus |
| Priority    | Job priority class        | priority field, sort on allocate |
| Preemption  | Controller-driven         | preempt handler (requeue)       |
| Lock        | K8s controller single-writer | host.lock_acquire around allocate |
| Persistence | etcd (K8s)                | KV (job metadata), state       |

## References

- [PLAN.md – migrating_kueue](../../../../PLAN.md)
- [Kueue](https://k8s.io/docs/concepts/scheduling/kueue/)
