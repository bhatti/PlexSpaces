# Distributed Task Queue - LockFacet Example (Python WASM with SDK)

Demonstrates **distributed task queue coordination** using **LockFacet** to ensure only one worker processes each job at a time.

**Real-world use case**: Distributed job processing, task scheduling, preventing duplicate work (similar to Celery, Sidekiq, Bull).

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md):

```python
from plexspaces import actor, state, handler, host

@actor
class TaskQueueCoordinator:
    job_queue: dict = state(default_factory=dict)
    
    @handler("submit")
    def submit_job(self, job_id: str = "", task_type: str = "") -> dict:
        self.job_queue[job_id] = {"task_type": task_type, "status": "pending"}
        host.info(f"Job submitted: {job_id}")
        return {"status": "ok", "job_id": job_id}
```

**Before SDK**: 183 lines with manual WIT interface  
**After SDK**: 110 lines with decorators

## How LockFacet Works

1. **LockFacet is attached** to the actor via `app-config.toml`
2. **LockFacet intercepts** lock messages: `acquire_lock`, `release_lock`, `renew_lock`
3. **Actor's handlers** only receive non-lock messages (submit, list, status)

### Task Queue Operations

| Operation | Message Type | Purpose |
|-----------|--------------|---------|
| Submit Job | `"submit"` | Add job to queue |
| Claim Job | `"try_acquire_lock"` | Worker claims job (intercepted) |
| Heartbeat | `"renew_lock"` | Keep lock alive (intercepted) |
| Complete | `"release_lock"` | Release after completion (intercepted) |
| List Jobs | `"list"` | List all jobs |
| Status | `"status"` | Get queue status |

## Quick Start

```bash
./build.sh    # Build WASM actor
./test.sh     # Test task queue operations
```

### Start Node

```bash
# Terminal 1: Start node
RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093

# Terminal 2: Run tests
./test.sh 8094
```

## Example Workflow

### 1. Submit a Job

```bash
curl -X POST "http://localhost:8094/api/v1/actors/task-queue-test/task-queue" \
  -H "Content-Type: application/json" \
  -d '{"msg_type":"submit","payload":{"job_id":"job-123","task_type":"send_email"}}'
```

### 2. Worker Claims Job

```bash
curl -X POST ... -d '{"msg_type":"try_acquire_lock","payload":{"lock_key":"job:job-123","holder_id":"worker-1","lease_duration_secs":300}}'
```

### 3. Worker Completes Job

```bash
curl -X POST ... -d '{"msg_type":"release_lock","payload":{"lock_key":"job:job-123","holder_id":"worker-1","version":"..."}}'
```

## Fault Tolerance

1. **Worker 1** claims job (acquires lock)
2. **Worker 1 crashes**
3. **Lock expires** (no renewal)
4. **Worker 2** claims job (lock available)

No job lost, no duplicate processing.

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `TaskQueueCoordinator` as PlexSpaces actor |
| `state()` | Defines `job_queue` as persistent dict |
| `@handler()` | Routes `submit`, `list`, `status` |
| `host.info()` | Logs job submissions |

## Files

| File | Description |
|------|-------------|
| `task_queue_actor.py` | Queue coordinator using SDK |
| `app-config.toml` | Config with LockFacet |
| `build.sh` | Build using `plexspaces-py build` |
| `test.sh` | Integration test |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Complete SDK reference
- [LockFacet Documentation](../../../../crates/facet/src/capabilities/locks.rs)
- [Registry Example](../registry/) - Service discovery with RegistryFacet
