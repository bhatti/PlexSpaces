# Actor Groups (Sharding) Example

**Purpose**: Demonstrate data-parallel horizontal scaling via sharding.

**Pattern**: Partition key → hash → shard_id → specific actor

## Overview

This example demonstrates the **Actor Groups (Sharding)** pattern for horizontal scaling. Messages are routed to specific shard actors based on a partition key, enabling high-throughput data operations.

## Features Demonstrated

1. **Sharded Counter Actors**: Multiple actor instances, each handling a subset of keys
2. **Router Actor**: Routes messages to the correct shard based on partition key
3. **Scatter-Gather Queries**: Query all shards and merge results
4. **ConfigBootstrap**: Erlang/OTP-style configuration loading
5. **CoordinationComputeTracker**: Metrics for coordination vs compute time

## Architecture

```
Router Actor
    │
    ├─→ Shard 0 (handles keys: user-1, user-5, ...)
    ├─→ Shard 1 (handles keys: user-2, ...)
    ├─→ Shard 2 (handles keys: user-3, ...)
    └─→ Shard 3 (handles keys: user-4, ...)
```

## Quick Start

```bash
# Run the example directly
cargo run --release --bin actor_groups_sharding

# Or use the run script
./scripts/run.sh

# Run tests and validation
./scripts/test.sh

# With custom configuration
SHARD_COUNT=8 cargo run --release --bin actor_groups_sharding
```

## Configuration

Configuration can be provided via:
1. `release.toml` file (in example directory)
2. Environment variables (prefixed with `SHARDING_`)

Example `release.toml`:
```toml
[sharding]
shard_count = 4
group_id = "counter-group"
```

## What It Shows

1. **Creating Shard Actors**: Multiple counter actors, one per shard
2. **Hash-Based Routing**: Partition key → hash → shard_id
3. **Message Routing**: Router sends messages to correct shard
4. **Scatter-Gather**: Query all shards for aggregate results
5. **Metrics**: Coordination vs compute time tracking

## Use Cases

✅ **Use Actor Groups when:**
- High-throughput workloads (millions of ops/sec)
- Horizontally scalable state (counters, sets, maps)
- Partitionable data (user IDs, session IDs, timestamps)
- Eventual consistency acceptable
- Need scatter-gather queries

❌ **Don't use when:**
- Need broadcast/coordination → Use Process Groups (Pub/Sub) instead
- Strong consistency required → Use single actor with transactions
- Small datasets → Overhead not worth it

## Key Takeaways

- **Actor Groups**: Horizontal scaling via sharding (data parallelism)
- **Partition Key**: Routes to specific shard (1-to-1 mapping)
- **Scatter-Gather**: Query all shards, merge results
- **Scaling**: Add/remove shards, rebalance data

## See Also

- `docs/GROUPS_COMPARISON.md` - When to use Actor Groups vs Process Groups
- `examples/simple/process_groups_pubsub` - Pub/Sub pattern example
