# Supervision Tree Example

**Purpose**: Demonstrates Erlang/OTP-style supervision trees with fault tolerance patterns.

## Overview

This example shows how to use PlexSpaces supervision trees to build fault-tolerant actor systems:

1. **Basic Supervisor**: Supervisor managing multiple worker actors
2. **Supervisor-of-Supervisors**: Hierarchical supervision tree
3. **Failure Recovery**: Automatic restart on actor failures
4. **Restart Strategies**: OneForOne, OneForAll, RestForOne

## Architecture

```
RootSupervisor (OneForOne)
  ├─ WorkerSupervisor (OneForAll)
  │   ├─ Worker1 (Permanent)
  │   ├─ Worker2 (Permanent)
  │   └─ Worker3 (Transient)
  └─ ServiceSupervisor (RestForOne)
      ├─ Service1 (Permanent)
      ├─ Service2 (Permanent)
      └─ Service3 (Temporary)
```

## Running the Example

```bash
# Run the example
cargo run --bin supervision_tree

# Run with tracing output
RUST_LOG=debug cargo run --bin supervision_tree

# Run tests
cargo test
```

## What This Demonstrates

- ✅ Creating supervisors with different strategies
- ✅ Adding workers to supervisors
- ✅ Hierarchical supervision (supervisor-of-supervisors)
- ✅ Automatic restart on failures
- ✅ Restart intensity tracking (max restarts within time window)
- ✅ Cascading shutdown (graceful shutdown of entire tree)
- ✅ Event monitoring (supervisor events)

## Key Concepts

### Supervision Strategies

- **OneForOne**: Only restart the failed child
- **OneForAll**: Restart all children when any child fails
- **RestForOne**: Restart the failed child and all started after it

### Restart Policies

- **Permanent**: Always restart on failure
- **Transient**: Restart only on abnormal exit
- **Temporary**: Never restart (let it die)

### Restart Intensity

- Maximum restarts within a time window
- Prevents infinite restart loops
- Escalates to parent supervisor when exceeded

## Example Output

```
INFO  supervision_tree: Starting supervision tree example
INFO  supervision_tree::supervisor: Adding child to supervisor supervisor_id="root" child_id="worker-1@localhost"
INFO  supervision_tree::supervisor: Child added to supervisor successfully supervisor_id="root" child_id="worker-1@localhost"
...
INFO  supervision_tree::supervisor: Handling child failure supervisor_id="root" child_id="worker-1@localhost" reason="simulated failure"
INFO  supervision_tree::supervisor: Child restarted successfully supervisor_id="root" child_id="worker-1@localhost" restart_count=1
...
INFO  supervision_tree::supervisor: Starting supervisor shutdown supervisor_id="root"
INFO  supervision_tree::supervisor: Supervisor shutdown completed supervisor_id="root"
```

## See Also

- [Supervision Crate Documentation](../../../crates/supervisor/README.md)
- [Supervision Review](../../../docs/SUPERVISION_REVIEW.md)
- [Erlang/OTP Supervisor Documentation](https://www.erlang.org/doc/design_principles/sup_princ.html)
