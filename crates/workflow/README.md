# PlexSpaces Workflow Orchestration

## Overview

The `plexspaces-workflow` crate provides durable workflow orchestration with Temporal/Step Functions-like patterns. Workflows are actors with journaling enabled, providing both durability and queryability.

## Features

- **Durable Workflow Execution**: Workflows persist state to journal for recovery
- **Metadata Storage**: SQL database for queryable workflow definitions and execution metadata
- **Multi-Database Support**: SQLite (testing/embedded) and PostgreSQL (production)
- **Optimistic Locking**: Version-based concurrency control for safe concurrent updates
- **Health Monitoring**: Heartbeat tracking for node health and stale workflow detection
- **Auto-Recovery**: Automatic resumption of interrupted workflows on node startup
- **Node Ownership**: Workflow ownership tracking for multi-node deployments

## Database Support

The workflow storage layer supports both SQLite and PostgreSQL:

### SQLite

**Use Cases:**
- Testing and development
- Embedded deployments
- Single-node setups

**Connection:**
```rust
use plexspaces_workflow::*;

// In-memory (for testing)
let storage = WorkflowStorage::new_in_memory().await?;

// File-based
let storage = WorkflowStorage::new_sqlite("sqlite://workflow.db").await?;
```

### PostgreSQL

**Use Cases:**
- Production multi-node deployments
- High-availability setups
- Distributed workflow execution

**Connection:**
```rust
use plexspaces_workflow::*;

// From connection string
let storage = WorkflowStorage::new_postgres(
    "postgresql://user:password@localhost/workflow_db"
).await?;
```

## Database-Specific SQL

The storage layer automatically handles database-specific SQL syntax:

- **Parameter Placeholders**: `?` for SQLite, `$1, $2, ...` for PostgreSQL
- **Timestamp Functions**: `CURRENT_TIMESTAMP` for SQLite, `NOW()` for PostgreSQL
- **Date Calculations**: `julianday()` for SQLite, `EXTRACT(EPOCH FROM ...)` for PostgreSQL
- **Row Identifiers**: `rowid` for SQLite, `ctid` for PostgreSQL
- **ON CONFLICT**: `excluded.column` for SQLite, `EXCLUDED.column` for PostgreSQL

## Optimistic Locking

All update operations support version-based optimistic locking to prevent race conditions:

```rust
// Update with version check
storage.update_execution_status_with_version(
    &execution_id,
    ExecutionStatus::Running,
    Some(expected_version)
).await?;

// If version mismatch, returns WorkflowError::ConcurrentUpdate
```

## Health Monitoring

Workflows track heartbeats for health monitoring:

```rust
// Update heartbeat
storage.update_heartbeat(&execution_id, "node-1").await?;

// Find stale workflows (not updated recently)
let stale = storage.list_stale_executions(
    300, // threshold in seconds
    vec![ExecutionStatus::Running]
).await?;
```

## Workflow Recovery

The storage layer supports workflow recovery scenarios:

### Node Crash Recovery

```rust
// Find workflows owned by a node
let running = storage.list_executions_by_status(
    vec![ExecutionStatus::Running],
    Some("node-1")
).await?;

// Transfer ownership to recovering node
for execution in running {
    storage.transfer_ownership(
        &execution.execution_id,
        "node-2", // new owner
        execution.version
    ).await?;
}
```

### Auto-Recovery on Startup

The `WorkflowRecoveryService` (in examples) provides automatic recovery:

```rust
let recovery_service = WorkflowRecoveryService::new(storage, node_id);
recovery_service.recover_on_startup(
    &config.recovery.stale_threshold,
    &config.recovery.heartbeat_interval
).await?;
```

## Testing

Integration tests are provided in `tests/storage_integration_test.rs`:

```bash
# Run all integration tests
cargo test --package plexspaces-workflow --test storage_integration_test

# Run with coverage
cargo tarpaulin --packages plexspaces-workflow --lib --tests
```

### Test Coverage

The integration tests cover:
- ✅ Basic CRUD operations (create, read, update)
- ✅ Optimistic locking and concurrent update detection
- ✅ Node ownership transfer
- ✅ Health monitoring (heartbeat updates)
- ✅ Stale workflow detection
- ✅ Signal handling (send, check, consume)
- ✅ Step execution lifecycle
- ✅ Recovery scenarios (node crash, concurrent transfer)
- ✅ Error cases (not found, version mismatch)

## Migration Files

Database migrations are in `migrations/`:

- `001_workflow_definitions.sql`: Workflow definition storage
- `002_workflow_executions.sql`: Execution metadata with version and heartbeat
- `003_workflow_execution_labels.sql`: Label storage for executions
- `004_step_executions.sql`: Step execution history
- `005_signals.sql`: Signal storage for workflow communication

Migrations are automatically applied when creating a new `WorkflowStorage` instance.

## API Reference

See the [API documentation](https://docs.rs/plexspaces-workflow) for detailed API reference.

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Workflows](../../docs/detailed-design.md#workflows) - Comprehensive workflow documentation
- [Use Cases](../../docs/use-cases.md) - Real-world workflow applications
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with workflows
- Implementation: `crates/workflow/src/`
- Tests: `crates/workflow/tests/`
- Proto definitions: `proto/plexspaces/v1/workflow/workflow.proto`

## Examples

See `examples/genomic-workflow-pipeline` and `examples/domains/order-processing` for complete examples of workflow orchestration with recovery.

