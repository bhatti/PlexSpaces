# Job Queue — Flow-Controlled Messaging Example

Demonstrates the outbox pattern with AIMD concurrency control for reliable,
flow-controlled message delivery to worker actors — including partitioned dispatch
for million-target scale-out.

## Features

- **ConcurrencyLimiter (AIMD)** — Adaptive concurrency: additive increase on success, multiplicative decrease on RESOURCE_EXHAUSTED
- **Bounded mailbox** — Receiver-side backpressure gate; `MailboxFull` → `DeliveryError::Capacity` → AIMD multiplicative decrease
- **OutboxRelay** — Async drain loop that reads from a channel and delivers at controlled rate
- **PartitionedOutboxRelay** — N independent partitions with hash routing for scalable fan-out
- **Scaling benchmark** — Sweeps over 1/3/6/12 workers measuring throughput, AIMD adaptation, and efficiency

## Architecture

```
Single relay:
  Producer → InMemoryChannel → OutboxRelay (AIMD) → Worker Actors (bounded mailbox)
                                                      MailboxFull → DeliveryError::Capacity → AIMD decrease

Partitioned relay (N=4):
  Producer → send(target, msg) → HashRouter → Partition K
                                                ├─ Partition 0: Channel → Limiter → Relay → deliver()
                                                ├─ Partition 1: Channel → Limiter → Relay → deliver()
                                                ├─ Partition 2: Channel → Limiter → Relay → deliver()
                                                └─ Partition 3: Channel → Limiter → Relay → deliver()
```

## Running

```bash
cargo run
# Or via test script:
./test.sh
```

## What it demonstrates

1. **Config validation** — `validate_config()` checks AIMD limits and batch size before use
2. **Single relay** — 3 workers with bounded mailbox, 50 jobs, AIMD adaptation, dispatcher status
3. **Scaling benchmark** — 1/3/6/12 workers × 100 jobs, throughput table with speedup/efficiency
4. **Partitioned relay** — 4 partitions × 200 jobs to 50 target actors, per-partition status breakdown

## Configuration

### PartitionedOutboxConfig (proto)

```protobuf
message PartitionedOutboxConfig {
  uint32 num_partitions = 1;    // >= 1, required; number of independent relay tasks
  OutboxConfig per_partition = 2; // applied to each partition's relay
  HashStrategy hash_strategy = 3; // MODULO (default) or CONSISTENT (reserved)
}
```

### Rust construction

```rust
use plexspaces_outbox::PartitionedOutboxRelay;
use plexspaces_proto::outbox::v1::{PartitionedOutboxConfig, OutboxConfig, ConcurrencyConfig};
use plexspaces_common::retry::default_retry_policy;

let config = PartitionedOutboxConfig {
    num_partitions: 4,   // panics if 0
    per_partition: Some(OutboxConfig {
        concurrency: Some(ConcurrencyConfig {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 50,
            strategy: 0, // AIMD
        }),
        retry_policy: Some(default_retry_policy()),
        relay_interval_ms: 5,   // poll interval per partition (ms)
        relay_batch_size: 20,   // max messages dequeued per cycle per partition
        storage_channel: None,  // ignored by PartitionedOutboxRelay (uses InMemoryChannel)
    }),
    hash_strategy: 0, // MODULO
};

let relay = PartitionedOutboxRelay::new("my-dispatcher", &config, delivery).await?;

// Enqueue: routes to partition via hash(target_actor_id) % N
relay.send("actor-123", msg).await?;

// Start all partition relay tasks
let handles = relay.run();

// ... wait for completion ...

// MUST stop and await before dropping
relay.stop();
for h in handles { h.await?; }

// Inspect per-partition metrics
let status = relay.status();
for (i, ps) in status.partition_statuses.iter().enumerate() {
    println!("partition {}: delivered={}, failed={}, aimd_limit={}",
        i, ps.delivered_count, ps.failed_count, ps.current_concurrency_limit);
}
```

## State Storage

| Component | State | Storage | Durability |
|-----------|-------|---------|------------|
| `OutboxRelay` (single) | Pending messages | Injected `Arc<dyn Channel>` (caller chooses: InMemory, SQLite, Postgres) | Depends on channel backend |
| `PartitionedOutboxRelay` | Pending messages per partition | **InMemoryChannel only** (created internally) | **Not durable** — process crash loses pending messages |
| `ConcurrencyLimiter` | Current AIMD limit, in-flight count | In-process `AtomicU32` | Lost on restart; reconverges from `initial_limit` |
| `RelayStats` | Delivered/failed/dead-letter counters | In-process `AtomicU64` | Lost on restart; observable via `status()` or metrics |

**For durable partitioned dispatch**, compose N `OutboxRelay` instances manually, each with a DB-backed channel, and use `HashRouter` for routing:

```rust
use plexspaces_outbox::{HashRouter, OutboxRelay, ConcurrencyLimiter};

let router = HashRouter::new(4);
let relays: Vec<Arc<OutboxRelay>> = (0..4).map(|i| {
    let channel = create_sqlite_channel(format!("partition-{}", i)).await;
    Arc::new(OutboxRelay::new(&config, channel, limiter.clone(), delivery.clone()))
}).collect();

// Route messages
let partition = router.route("actor-123") as usize;
relays[partition].channel.send(msg).await?;
```

## Gotchas

1. **`num_partitions` must be >= 1** — panics at construction if 0 (matches proto `gte: 1` validation)
2. **`storage_channel` is ignored** — `PartitionedOutboxRelay` always creates `InMemoryChannel` per partition; the `OutboxConfig.storage_channel` field has no effect. For durable storage, compose `OutboxRelay` instances manually (see above).
3. **Caller owns task lifecycle** — `run()` returns `Vec<JoinHandle<()>>`; caller MUST call `stop()` and await all handles before dropping. Failing to do so leaks N background tasks.
4. **Metrics aggregate across partitions** — all N relays emit to the same `plexspaces_outbox_*` metric names (no per-partition labels). Use `status()` for per-partition visibility.
5. **Changing partition count** — channels are transient; to resize: create a new relay with desired N, drain the old one, switch over. No data movement or rebalancing needed.
6. **Hash routing is deterministic** — same `target_actor_id` always maps to the same partition (FNV-1a mod N). Per-recipient ordering is preserved within a partition.
7. **Channel capacity** — each partition's channel capacity is `max(relay_batch_size, 100) * 10`. If you send faster than the relay drains, the channel will reject with a full error.

## Testing

```bash
# Unit tests (fast, during development):
cargo test -p plexspaces-outbox --lib

# Partitioned relay tests only:
cargo test -p plexspaces-outbox --lib -- partitioned_relay

# Hash router tests only:
cargo test -p plexspaces-outbox --lib -- hash_router

# Full test suite (before commit):
make test

# This example's test script:
./test.sh    # build + run + verify 4 assertions
```

### Test coverage

| Test | What it verifies |
|------|-----------------|
| `test_all_messages_delivered` | 100 messages across 4 partitions all reach delivery callback |
| `test_status_aggregation` | `status()` correctly sums delivered/failed across partitions |
| `test_same_target_same_partition` | Same target routes to exactly one partition (ordering guarantee) |
| `test_zero_partitions_panics` | 0 partitions rejected with panic (proto contract enforcement) |
| `test_failure_isolated_to_partition` | RESOURCE_EXHAUSTED failures in one partition don't affect others |

## Sample output

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Partitioned relay: 4 partitions × 200 jobs to 50 targets
  (hash routing preserves per-recipient ordering)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Enqueue 200 jobs  : 0 ms
  Total elapsed    : 1 ms
  Partitions       : 4
  Total delivered  : 200
  Total failed     : 0
  Success rate     : 100.0%

  Per-partition breakdown:
  ┌───────────┬───────────┬────────┬────────────┐
  │ Partition │ Delivered │ Failed │ AIMD Limit │
  ├───────────┼───────────┼────────┼────────────┤
  │         0 │        52 │      0 │         50 │
  │         1 │        48 │      0 │         50 │
  │         2 │        48 │      0 │         50 │
  │         3 │        52 │      0 │         50 │
  └───────────┴───────────┴────────┴────────────┘

  ✓ Partitioned relay assertions passed
```

## References

- [Architecture — Outbox & Flow Control](../../../../docs/architecture.md)
- [Detailed Design — Flow-Controlled Messaging](../../../../docs/detailed-design.md)
- [Getting Started](../../../../docs/getting-started.md)
