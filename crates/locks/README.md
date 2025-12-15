# Locks - Distributed Lock/Lease Coordination

**Purpose**: Provides distributed lock/lease coordination for background schedulers and other coordination tasks. Uses version-based optimistic locking for atomic operations.

## Overview

This crate is used internally by:
- **Background Scheduler**: Lease-based coordination to ensure only one scheduler processes requests at a time
- **Future Coordination Tasks**: Any component that needs distributed locking

## Design Decisions

- **Version-based optimistic locking**: Prevents lost updates in concurrent scenarios
- **Timestamp-based expiration**: Stale lock detection for fault tolerance
- **Heartbeat mechanism**: Renews lease expiration timestamp periodically
- **Backend-agnostic**: Supports SQL (PostgreSQL/SQLite) or Redis backends

## Backend Support

- **InMemory**: HashMap-based (always available, for testing)
- **SQLite**: Persistent, single-node (feature: `sqlite-backend`)
- **PostgreSQL**: Distributed, multi-node (feature: `postgres-backend`)
- **Redis**: Distributed with native TTL (feature: `redis-backend`)

## Usage Examples

### Basic Usage

```rust
use plexspaces_locks::{LockManager, memory::MemoryLockManager};
use plexspaces_proto::locks::prv::{AcquireLockOptions, RenewLockOptions, ReleaseLockOptions};

let manager = MemoryLockManager::new();

// Acquire lock
let lock = manager.acquire_lock(AcquireLockOptions {
    lock_key: "scheduler:background:lease".to_string(),
    holder_id: "node-1".to_string(),
    lease_duration_secs: 30,
    additional_wait_time_ms: 0,
    refresh_period_ms: 100,
    metadata: Default::default(),
}).await?;

// Renew lock (heartbeat)
let renewed = manager.renew_lock(RenewLockOptions {
    lock_key: "scheduler:background:lease".to_string(),
    holder_id: "node-1".to_string(),
    version: lock.version.clone(),
    lease_duration_secs: 30,
    metadata: Default::default(),
}).await?;

// Release lock
manager.release_lock(ReleaseLockOptions {
    lock_key: "scheduler:background:lease".to_string(),
    holder_id: "node-1".to_string(),
    version: renewed.version,
    delete_lock: false,
}).await?;
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_keyvalue`: Key-value storage backend
- `sqlx`: SQL backends (optional)
- `redis`: Redis backend (optional)

## Dependents

This crate is used by:
- `plexspaces_scheduler`: Background scheduler uses locks for coordination
- Other coordination tasks: Any component needing distributed locking

## References

- Implementation: `crates/locks/src/`
- Tests: `crates/locks/tests/`
- Proto definitions: `proto/plexspaces/v1/locks.proto`

