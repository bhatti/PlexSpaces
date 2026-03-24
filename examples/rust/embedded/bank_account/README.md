# Bank Account - Durable Actor Example

**Real-world use case**: Banking, wallets, financial ledgers - where account balances and transaction history must survive crashes and restarts.

**Pattern**: Durable actors with journaling and deterministic replay.

## Overview

This example demonstrates durable actors with journaling and deterministic replay using a bank account use case. All operations (deposits, withdrawals) are journaled before execution, ensuring exactly-once semantics and providing a complete audit trail.

## Architecture

### Journaling Pattern

1. **Journaling**: All operations persisted before execution
   - Ensures exactly-once semantics: no duplicate operations
   - Provides audit trail: complete transaction history
   - Enables deterministic replay: state recovered from journal on restart

2. **Checkpointing**: Periodic state snapshots for fast recovery
   - Checkpoint every N operations (configurable)
   - Recovery from checkpoint is 90%+ faster than full replay
   - Checkpoints include full account state (balance, transaction log)

3. **SDK Patterns**: Uses SDK annotations and helpers to minimize boilerplate
   - `#[gen_server_actor(facets = ["durability"])]` - Declares durable GenServer behavior
   - `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
   - `#[handler("deposit")]` / `#[handler("withdraw")]` - Transaction handlers
   - `spawn_with_storage()` - SDK helper over the framework-owned durability spawn path
   - `GenServerRef.call()` - Request-reply messaging (wraps ActorRef.ask())

## SDK Features Demonstrated

- `#[gen_server_actor(facets = ["durability"])]` - Declares durable GenServer behavior
- `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
- `#[handler("deposit")]` / `#[handler("withdraw")]` - Transaction handlers
- `spawn_with_storage()` - SDK helper over the framework-owned durability spawn path
- `GenServerRef.call()` - Request-reply messaging (wraps ActorRef.ask())

## Durability Features

- **Journaling**: All operations persisted before execution
- **Checkpointing**: Periodic state snapshots for fast recovery
- **Deterministic Replay**: State recovered from journal on restart
- **Exactly-Once Semantics**: No duplicate operations

## Quick Start

```bash
cd examples/rust/embedded/bank_account

# Build
cargo build

# Run
cargo run

# Run with debug logging
RUST_LOG=bank_account=debug cargo run
```

## Expected Output

```
╔════════════════════════════════════════════════════════════════╗
║     Bank Account - Durable Actor with Journaling              ║
╚════════════════════════════════════════════════════════════════╝

Step 1: Create PlexSpaces Node
  ✓ Node 'bank-node' created

Step 2: Setting up journal storage
  ✓ Journal storage created
  Storage: In-memory SQLite (use file-based for production)

Step 3: Spawn durable bank account actor
  ✓ Account 'account-123@bank-node' spawned
  Durability facet attached (journaling enabled)

Step 4: Process 1000 Transactions (All Journaled)
  Initial deposit: $10000.00
  Processed 1000 transactions
  Final balance: $39774.00

Step 5: Simulate Crash and Recovery
  ✓ Journal contains 1001 transactions
  ✓ State persisted (would be restored on restart)

PERFORMANCE METRICS & BENCHMARKS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
COORDINATION vs COMPUTATION ANALYSIS
Total Time:                    XXXXms
Coordination Time:             XXXXms (XX%)
Computation Time:              XXXXms (XX%)
Granularity Ratio:             XX.XXx
Efficiency:                    XX.X%

BENCHMARK METRICS
Transactions Processed:        1000
Transactions/Second:           XXX.XX
Avg Latency per Transaction:   X.XXms
Journal Entries:               1001
```

## Real-World Use Cases

- **Banking**: Account balances, transaction history
- **Wallets**: Cryptocurrency balances, payment processing
- **Financial Ledgers**: Audit trails, compliance

## Design Principles

- **Core Functionality**: Lives in main crates (DurabilityFacet, JournalStorage)
- **SDK Role**: Provides decorators/helpers to simplify usage
- **No Hacks**: Proper trait usage, no cyclic dependencies
- **Observability**: CoordinationComputeTracker for metrics
- **Tenant Isolation**: Explicit RequestContext with tenant/namespace

## See Also

- [Player Session](../player_session/) - Virtual actor with durability
- [Reminders](../reminders/) - Durable timers
- [Architecture Docs](../../../../docs/architecture.md)
- [Durability Docs](../../../../docs/durability.md)
