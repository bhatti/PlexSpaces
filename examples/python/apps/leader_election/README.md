# Leader Election - Distributed Lock Example (Python WASM with SDK)

> **Status: buggy/incomplete.** This example may not run correctly end-to-end; see PROJECT_TRACKER.md.

Demonstrates **leader election** using a single distributed lock so that one actor holds the "leader" role at a time.

**Real-world use cases**:
- Cron singleton (one node runs scheduled jobs)
- Task scheduler (single active coordinator)
- Single active consumer (one consumer drains a queue)
- Cluster coordinator / master election

## Leader Election Pattern

```
┌─────────────┐     try_lead     ┌─────────────────┐
│  Candidate  │ ───────────────►│  Leader Lock    │
│  (actor 1)  │ ◄───────────────│  (distributed)  │
└─────────────┘   leader: true   └────────┬────────┘
                                           │
      ┌────────────────────────────────────┼────────────────────────────────────┐
      │                                    │                                    │
      ▼                                    ▼                                    ▼
┌───────────┐                        ┌───────────┐                        ┌───────────┐
│ Candidate │                        │ Candidate │                        │ Candidate │
│  (actor 2)│   try_lead → false     │  (actor 3)│   try_lead → false     │   (N)     │
└───────────┘   (lock held)           └───────────┘                        └───────────┘
```

Only one actor holds the leader lock; others get `leader: false` until the leader releases.

## Lock APIs Used

Locks are **lease-based** (see [Implementing Distributed Locks with Databases](https://shahbhat.medium.com/implementing-distributed-locks-mutex-and-semaphore-with-databases-b02545cef47a)): the leader must **renew the lease** periodically to keep holding the lock; if it stops renewing (e.g. crash), the lease expires and another candidate can acquire.

| API | Usage | Description |
|-----|-------|-------------|
| `host.lock_acquire(lock_id, timeout_ms)` | Try or wait for leader lock | Returns lock version on success; `timeout_ms=0` = try-acquire (no wait) |
| `host.lock_renew(lock_id, lock_version, lease_duration_secs)` | Renew lease (heartbeat) | Leader calls periodically to extend expiration; returns new version (use for next renew/release) |
| `host.lock_release(lock_id, lock_version)` | Release leader lock | Call when stepping down so another can become leader |

## Manual testing (recommended)

Use **deploy once**, then **compete from two terminals**. One becomes leader; the other waits. Kill the leader (Ctrl+C) and the other acquires after lease expiry.

### 1. Start the node

```bash
# Terminal 1: start node (from repo root)
./scripts/server.sh
```

### 2. Deploy once

```bash
# Terminal 2: deploy both term1 and term2 apps (run once)
cd examples/python/apps/leader_election
./deploy.sh 8091
```

### 3. Compete from two terminals

**Important:** Use the **same port** in both terminals so both talk to the **same node** and contend for the **same lock**. If you use different ports (different nodes), each node has its own lock backend and both would incorrectly acquire.

```bash
# Terminal 2: run competitor term1 (same port as term2)
./compete.sh term1 8091

# Terminal 3: run competitor term2 (same port as term1)
./compete.sh term2 8091
```

- **One terminal** acquires the lock and prints "Acquired lock → leader (term1)" (or term2), then renews every 5s until you stop it.
- **The other terminal** prints "Acquiring lock..." every 5s for up to 300s.
- **Kill the leader** (Ctrl+C in the leader terminal). After lease expiry (~30s), the other terminal acquires and becomes leader.

### Scripts

| Script | Purpose |
|--------|---------|
| `deploy.sh [port]` | Deploy app for **term1** and **term2** (run once). Builds WASM if needed. Default port 8091. |
| `compete.sh term1\|term2 [port]` | Compete for the **same lock**. No deploy. Try-acquire → if leader, renew every 5s until killed; else retry every 5s (up to 300s). |
| `test.sh [port]` | One-shot flow (deploy + acquire + renew + release). Optional. |

## Operations

| Operation | Payload | Description |
|-----------|---------|-------------|
| `try_lead` | `{}` or `{"candidate_id": "owner-id"}` | Try to become leader (non-blocking). Returns `leader: true/false`. `candidate_id` is for display (e.g. two-terminal test). |
| `acquire_lead` | `{"timeout_ms": 5000}` or `{"candidate_id": "owner-id"}` | Attempt to become leader, waiting up to timeout_ms. |
| `renew_lead` | `{"lease_duration_secs": 30}` or `{"candidate_id": "owner-id"}` | Renew lease on the leader lock (heartbeat). Leader should call periodically to keep holding the lock. Returns `renewed: true/false`. |
| `release_lead` | `{}` or `{"candidate_id": "owner-id"}` | Release leadership so another candidate can become leader. |
| `status` | `{}` or `{"candidate_id": "owner-id"}` | Report whether this instance is currently the leader. |

## Example Flow

```python
# Candidate 1 tries to become leader (pass candidate_id in payload for two-actor demo)
try_lead(payload={"candidate_id": "actor1"})  # → {"leader": true, "candidate_id": "actor1"}

# Candidate 2 tries (lock held by 1)
try_lead(payload={"candidate_id": "actor2"})  # → {"leader": false, "candidate_id": "actor2"}

# Candidate 1 releases
release_lead(payload={"candidate_id": "actor1"})  # → {"leader": false, "message": "released"}

# Candidate 2 tries again
try_lead(payload={"candidate_id": "actor2"})  # → {"leader": true, "candidate_id": "actor2"}
```

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|----------------|
| `@actor` | GenServer request-reply |
| `state()` | Track candidate_id and lock_version (whether we hold leader lock) |
| `@handler()` | try_lead, acquire_lead, renew_lead, release_lead, status |
| `@init_handler` | Set candidate_id from config |
| `host.lock_acquire()` | Acquire leader lock (try or wait) |
| `host.lock_renew()` | Renew lease (heartbeat) while holding lock |
| `host.lock_release()` | Release leader lock |

## Why Leader Election?

1. **Single active**: Only one instance runs the "leader" workload (cron, coordinator).
2. **Failover**: When the leader releases or crashes (lock TTL), another acquires and becomes leader.
3. **No central broker**: Uses the same distributed lock backend as the rest of PlexSpaces.

## Files

| File | Description |
|------|-------------|
| `leader_election_actor.py` | Leader election actor using SDK and host locks; uses payload `candidate_id` as lock holder |
| `build.sh` | Build using `plexspaces-py build` |
| `deploy.sh` | Deploy app for term1 and term2 once; then use compete.sh in two terminals |
| `compete.sh` | Compete for lock (no deploy); one wins, renews every 5s; kill leader so other wins |
| `test.sh` | One-shot flow (optional) |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md) - SDK documentation
- [SDK Guide](../../../../docs/sdk.md) - Lock API reference
- [Payment Handler Example](../payment_handler/) - Locks for critical section (refund)
- [Durability](../../../../docs/durability.md) - Checkpoint and journaling
