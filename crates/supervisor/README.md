# Supervisor - Erlang/OTP-Style Supervision Trees

**Purpose**: Provides supervision trees for fault-tolerant actor systems, following Erlang/OTP supervision patterns.

## Overview

The Supervisor crate implements Erlang/OTP supervision patterns for managing actor lifecycles and handling failures gracefully.

### Key Concepts

- **Supervision Tree**: Hierarchical structure of supervisors and workers
- **Restart Strategies**: How to handle child failures (one-for-one, one-for-all, rest-for-one, simple-one-for-one)
- **Restart Policies**: When to restart (always, transient, temporary)
- **Restart Intensity**: Maximum restarts within a time window

## Supervision Strategies

### One-for-One

Only restart the failed child:

```rust
let supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::OneForOne)
    .build();

supervisor.add_child(child_spec).await?;
// If child1 fails, only child1 is restarted
```

### One-for-All

Restart all children when any child fails:

```rust
let supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::OneForAll)
    .build();

// If child1 fails, all children (child1, child2, child3) are restarted
```

### Rest-for-One

Restart the failed child and all children started after it:

```rust
let supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::RestForOne)
    .build();

// Children started in order: child1, child2, child3
// If child2 fails, child2 and child3 are restarted (child1 is preserved)
```

### Simple-One-for-One

Dynamic worker pool (all children are identical):

```rust
let supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::SimpleOneForOne)
    .with_template(child_template)
    .build();

// Add workers dynamically
supervisor.add_worker().await?;
supervisor.add_worker().await?;
// If one worker fails, only that worker is restarted
```

## Restart Policies

### Always

Always restart on failure:

```rust
let child_spec = ChildSpec::new("worker")
    .with_restart_policy(RestartPolicy::Always);
```

### Transient

Restart only on abnormal exit (not on normal exit):

```rust
let child_spec = ChildSpec::new("worker")
    .with_restart_policy(RestartPolicy::Transient);
```

### Temporary

Never restart (let it die):

```rust
let child_spec = ChildSpec::new("worker")
    .with_restart_policy(RestartPolicy::Temporary);
```

## Restart Intensity

Limit restarts to prevent restart loops:

```rust
let supervisor = Supervisor::new()
    .with_max_restarts(5)        // Max 5 restarts
    .with_restart_window(Duration::from_secs(60))  // Within 60 seconds
    .build();

// If more than 5 restarts occur within 60 seconds, supervisor stops all children
```

## Testing Strategy

### Test Hierarchy

1. **Unit Tests** (in `src/mod.rs`): Test individual components in isolation
2. **Integration Tests** (in `tests/`): Test components working together
3. **End-to-End Tests** (in `examples/`): Real-world scenarios

### Test Coverage Goals

- `supervisor/mod.rs`: **95%** (core logic)
- `supervisor/tests/`: **90%** (integration)
- `examples/byzantine/`: **80%** (E2E demonstration)

### Known Issues

Some tests are currently hanging (indicate bugs in restart/failure handling):
- `test_handle_failure_one_for_one` - Waits for `ChildRestarted` event that never arrives
- `test_temporary_restart_policy` - Hangs after failure
- `test_max_restarts_exceeded` - Hangs on repeated failures
- `test_supervisor_stats` - Hangs waiting for stats update

**Status**: These need debugging to fix event flow and restart logic.

## Usage Examples

### Basic Supervisor

```rust
use plexspaces_supervisor::{Supervisor, ChildSpec, SupervisionStrategy};

let supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::OneForOne)
    .with_max_restarts(5)
    .with_restart_window(Duration::from_secs(60))
    .build();

let child_spec = ChildSpec::new("worker")
    .with_restart_policy(RestartPolicy::Always);

supervisor.add_child(child_spec).await?;
```

### Supervision Tree

```rust
// Top-level supervisor
let root_supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::OneForAll)
    .build();

// Add sub-supervisor
let sub_supervisor = Supervisor::new()
    .with_strategy(SupervisionStrategy::OneForOne)
    .build();

root_supervisor.add_child(ChildSpec::new("sub_supervisor")
    .with_supervisor(sub_supervisor)).await?;
```

## Running Tests

```bash
# Run all supervisor tests
cargo test -p plexspaces-supervisor

# Run individual test
cargo test -p plexspaces-supervisor test_supervisor_creation

# Run with output
cargo test -p plexspaces-supervisor -- --nocapture

# Check coverage
cargo tarpaulin -p plexspaces-supervisor --exclude-files 'crates/proto/*' --out Html
```

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Supervision](../../docs/detailed-design.md#supervision) - Comprehensive supervision documentation
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with supervision
- [Erlang/OTP Supervisor Pattern](https://www.erlang.org/doc/design_principles/sup_princ.html)
- Implementation: `crates/supervisor/src/mod.rs`
- Tests: `crates/supervisor/src/mod.rs` (unit tests)

