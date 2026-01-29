# Distributed Task Queue - LockFacet Example (Python WASM)

Demonstrates **distributed task queue coordination** using **LockFacet** to ensure only one worker processes each job at a time.

**Real-world use case**: Distributed job processing, task scheduling, preventing duplicate work across a cluster (similar to Celery, Sidekiq, Bull).

## What This Example Shows

This example demonstrates the **power of distributed locks** with:
- ✅ **Verbose output** showing all lock operations
- ✅ **Server-side logs** showing LockFacet intercepting operations
- ✅ **Concurrent access scenarios** showing lock contention
- ✅ **Multiple workers competing** for the same job
- ✅ **Lock acquisition, renewal, and release** with detailed logging

## The Problem

In a distributed system with multiple workers, you need to ensure:
- **No duplicate processing**: Only one worker processes each job
- **Fault tolerance**: If a worker crashes, another can pick up the job
- **Heartbeat/renewal**: Long-running jobs keep their lock alive
- **Contention handling**: Multiple workers competing for the same job

## How LockFacet Solves It

1. **Job Submission**: Tasks are submitted to the queue
2. **Lock Acquisition**: Worker acquires lock on job ID before processing
3. **Lock Renewal**: Worker renews lock periodically (heartbeat) during processing
4. **Lock Release**: Worker releases lock when job completes
5. **Automatic Recovery**: If worker crashes, lock expires and another worker can grab the job

## Real-World Use Cases

### 1. **Background Job Processing**
- Email sending, report generation, image processing
- Lock key: `"job:email-{user_id}"`
- Lease: 5 minutes (renewed every 30 seconds)

### 2. **Scheduled Task Execution**
- Cron jobs, periodic reports, data sync
- Lock key: `"scheduled:report-daily"`
- Lease: 1 hour (renewed every 5 minutes)

### 3. **Distributed Data Processing**
- ETL pipelines, batch processing, file processing
- Lock key: `"batch:process-file-{file_id}"`
- Lease: 30 minutes (renewed every 2 minutes)

### 4. **Resource Coordination**
- Database migrations, cache warming, index rebuilding
- Lock key: `"migration:v1-to-v2"`
- Lease: 2 hours (renewed every 10 minutes)

## How LockFacet Works

1. **LockFacet is attached** to the actor via `app-config.toml` (ChildSpec facets)
2. **LockFacet intercepts** messages with types: `"acquire_lock"`, `"release_lock"`, `"renew_lock"`, `"try_acquire_lock"`, `"get_lock"`
3. **LockFacet uses real LockManager** from ServiceLocator (configured via node-config/runtimeconfig, **not hardcoded**)
4. **Actor's handle() method** is never called for intercepted messages - facet handles them directly
5. **Production-grade**: Uses real distributed lock backend (SQLite, DynamoDB, Redis, etc.) based on node configuration

## Task Queue Operations

| Operation | Message Type | Purpose | Example |
|-----------|--------------|---------|---------|
| **Submit Job** | `"submit"` | Add job to queue | `{"job_id":"job-123","task_type":"send_email","payload":{...}}` |
| **Claim Job** | `"try_acquire_lock"` | Worker tries to claim job | `{"lock_key":"job:job-123","holder_id":"worker-1","lease_duration_secs":300}` |
| **Heartbeat** | `"renew_lock"` | Keep job lock alive | `{"lock_key":"job:job-123","holder_id":"worker-1","version":"...","lease_duration_secs":300}` |
| **Complete Job** | `"release_lock"` | Release job after completion | `{"lock_key":"job:job-123","holder_id":"worker-1","version":"..."}` |
| **Check Status** | `"get_lock"` | Check if job is being processed | `"job:job-123"` |
| **List Jobs** | `"list"` | List all jobs in queue | `{}` |

## Quick Start

```bash
./build.sh    # Build WASM actor
./test.sh      # Test task queue operations (requires running node)
```

### Start Node

```bash
# From workspace root
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093
```

Then in another terminal:

```bash
./test.sh 8094  # HTTP port is gRPC port + 1
```

## Running the Example

```bash
cd examples/python/apps/task-queue
./build.sh
./test.sh
```

### What to Watch For

**In the test output**, you'll see:
- ✅ Lock acquisition with lock key, holder, version, and lease duration
- ❌ Lock contention failures (multiple workers trying to claim the same job)
- 🔄 Lock renewal (heartbeat) operations
- 🔓 Lock release operations
- 📊 Queue status showing pending/processing/completed jobs

**In the server logs** (when running with `RUST_LOG=info`), you'll see:
- `🔒 LockFacet: Lock acquired` - When a worker successfully claims a job
- `⚔️  LockFacet: Lock contention` - When a worker tries to claim an already-locked job
- `🔄 LockFacet: Lock renewed (heartbeat)` - When a worker renews its lock
- `🔓 LockFacet: Lock released` - When a worker completes a job
- `🔍 LockFacet: Lock queried` - When checking lock status

**Example server log output:**
```
INFO LockFacet: Lock acquired lock_key="job:job-1" holder_id="worker-1" version="01J..." lease_secs=300
WARN LockFacet: Lock contention - lock already held by another worker lock_key="job:job-1" holder_id="worker-2" current_holder="worker-1"
INFO LockFacet: Lock renewed (heartbeat) lock_key="job:job-1" holder_id="worker-1" version="01J..." lease_secs=300
INFO LockFacet: Lock released lock_key="job:job-1" holder_id="worker-1"
```

### Running with Verbose Logging

To see detailed server-side logs, you **MUST** set `RUST_LOG` environment variable:

```bash
# Terminal 1: Start node with info-level logging (REQUIRED for logs)
RUST_LOG=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093

# Or more specific (only facet logs):
RUST_LOG=plexspaces_facet=info cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093

# Terminal 2: Run the test
cd examples/python/apps/task-queue
./test.sh
```

**Important**: Without `RUST_LOG=info`, server-side logs will NOT appear. The default log level is `info`, but you must explicitly set `RUST_LOG` to see logs.

You'll see both:
- **Test output**: Formatted, colored output showing operations (in Terminal 2)
- **Server logs**: Detailed LockFacet operations with emojis (in Terminal 1)

**Alternative**: Use `./test-with-logs.sh` which provides additional guidance on viewing logs.

## Example Workflow

### 1. Submit a Job

```bash
curl -X POST "http://localhost:8094/api/v1/actors/internal/system/task-queue" \
  -H "Content-Type: application/json" \
  -d '{
    "msg_type": "submit",
    "payload": {
      "job_id": "job-123",
      "task_type": "send_email",
      "payload": {"to": "user@example.com", "subject": "Welcome"}
    }
  }'
```

### 2. Worker Claims Job (Non-blocking)

```bash
curl -X POST "http://localhost:8094/api/v1/actors/internal/system/task-queue" \
  -H "Content-Type: application/json" \
  -d '{
    "msg_type": "try_acquire_lock",
    "payload": {
      "lock_key": "job:job-123",
      "holder_id": "worker-1",
      "lease_duration_secs": 300
    }
  }'
```

**Response if successful:**
```json
{
  "lock_key": "job:job-123",
  "holder_id": "worker-1",
  "version": "01J...",
  "lease_duration_secs": 300
}
```

**Response if already claimed:**
```json
{
  "acquired": false,
  "reason": "Lock already held by another worker"
}
```

### 3. Worker Renews Lock (Heartbeat)

```bash
curl -X POST "http://localhost:8094/api/v1/actors/internal/system/task-queue" \
  -H "Content-Type: application/json" \
  -d '{
    "msg_type": "renew_lock",
    "payload": {
      "lock_key": "job:job-123",
      "holder_id": "worker-1",
      "version": "01J...",
      "lease_duration_secs": 300
    }
  }'
```

### 4. Worker Completes Job

```bash
curl -X POST "http://localhost:8094/api/v1/actors/internal/system/task-queue" \
  -H "Content-Type: application/json" \
  -d '{
    "msg_type": "release_lock",
    "payload": {
      "lock_key": "job:job-123",
      "holder_id": "worker-1",
      "version": "01J..."
    }
  }'
```

## Fault Tolerance

### Worker Crash Scenario

1. **Worker 1** claims job `job-123` (acquires lock with 5-minute lease)
2. **Worker 1** starts processing (sends email, processes file, etc.)
3. **Worker 1 crashes** (network issue, OOM, etc.)
4. **Lock expires** after 5 minutes (no renewal)
5. **Worker 2** tries to claim job → succeeds (lock expired)
6. **Worker 2** processes the job

This ensures **no job is lost** and **no duplicate processing** occurs.

## LockFacet Configuration

The LockFacet is attached via `app-config.toml`:

```toml
[[supervisor.children.facets]]
type = "locks"
priority = 50
config = {}
```

The LockFacet automatically:
- Gets LockManager from ServiceLocator (configured via node-config/runtimeconfig)
- Intercepts lock operation messages
- Uses the real distributed lock backend (not hardcoded MemoryLockManager)

## Files

| File | Description |
|------|-------------|
| `task_queue_actor.py` | Task queue coordinator actor (handles job submission, listing) |
| `app-config.toml` | Application config with LockFacet attachment |
| `build.sh` | Build WASM |
| `test.sh` | Test task queue operations |

## See Also

- [Locks Documentation](../../../../crates/locks/README.md) - Full locks implementation (Rust)
- [LockFacet Documentation](../../../../crates/facet/src/capabilities/locks.rs) - LockFacet implementation
- [Python WASM Guide](../../README.md) - Python WASM development
- [Process Groups Example](../process_groups/) - Pub/sub coordination
- [Registry Example](../registry/) - Service discovery

## Comparison to Other Systems

| Feature | PlexSpaces LockFacet | Celery | Sidekiq | Bull |
|---------|---------------------|--------|---------|------|
| Distributed locks | ✅ | ❌ | ❌ | ✅ |
| Automatic recovery | ✅ | ✅ | ✅ | ✅ |
| Heartbeat/renewal | ✅ | ✅ | ✅ | ✅ |
| Non-blocking claim | ✅ | ✅ | ✅ | ✅ |
| Configurable backend | ✅ | ❌ | ❌ | ✅ |
